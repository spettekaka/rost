use core::ptr;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::consts::{PAGE_ORDER, PAGE_SIZE};

pub const MAX_PAGES: usize = 65_536; // 256 MiB
const BITMAP_WORDS: usize = MAX_PAGES.div_ceil(64);

/// Atomic page allocator
///
/// Uses a bitmap internally
pub struct AtomicPageAllocator {
    base_addr: AtomicUsize,
    bitmap: [AtomicU64; BITMAP_WORDS],
    search_hint: AtomicUsize,
    total_pages: AtomicUsize,
    used_pages: AtomicUsize,
}

impl Default for AtomicPageAllocator {
    fn default() -> Self {
        AtomicPageAllocator::new()
    }
}

impl AtomicPageAllocator {
    pub const fn new() -> Self {
        Self {
            base_addr: AtomicUsize::new(0),
            bitmap: [const { AtomicU64::new(0) }; BITMAP_WORDS],
            search_hint: AtomicUsize::new(0),
            total_pages: AtomicUsize::new(0),
            used_pages: AtomicUsize::new(0),
        }
    }

    pub fn init(&self, base_addr: usize, end_addr: usize) {
        // Round base_addr up to the nearest multiple of PAGE_SIZE.
        let start = base_addr.next_multiple_of(PAGE_SIZE);
        // Round end_addr down to the nearest multiple of PAGE_SIZE.
        let end = end_addr & !(PAGE_SIZE - 1);
        assert!(end > start, "Invalid page pool supplied");

        let total = ((end - start) / PAGE_SIZE).min(MAX_PAGES);
        self.base_addr.store(start, Ordering::Relaxed);
        self.search_hint.store(0, Ordering::Relaxed);
        self.total_pages.store(total, Ordering::Relaxed);
        self.used_pages.store(0, Ordering::Relaxed);

        self.bitmap
            .iter()
            .for_each(|x| x.store(0, Ordering::Relaxed));

        // Mark traliing pages as taken (1)
        if total % 64 != 0 {
            let mask = !0u64 << (total % 64);
            self.bitmap[total / 64].store(mask, Ordering::Relaxed);
        }
        let used_words = total.div_ceil(64);
        for word in &self.bitmap[used_words..] {
            word.store(u64::MAX, Ordering::Relaxed);
        }
    }

    pub fn alloc(&self, pages: usize) -> Option<*mut u8> {
        match pages {
            0 => None,
            1 => self.alloc_page(),
            n => self.alloc_pages(n),
        }
    }

    fn alloc_page(&self) -> Option<*mut u8> {
        let start_page = self.search_hint.load(Ordering::Relaxed);

        let total = self.total_pages.load(Ordering::Relaxed);
        let active_words = total.div_ceil(64);

        for offset in 0..active_words {
            let word_idx = (start_page + offset) % active_words;

            let atom = &self.bitmap[word_idx];
            let mut word = atom.load(Ordering::Relaxed);

            // Loop incase another thread takes the last page for this word
            while word != u64::MAX {
                let page = word.trailing_ones() as usize;
                let page_idx = word_idx * 64 + page;

                if page_idx >= total {
                    break;
                }

                let mask = 1u64 << page;

                match atom.compare_exchange_weak(
                    word,
                    word | mask,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        if (word | mask) == u64::MAX {
                            let _ = self.search_hint.compare_exchange_weak(
                                word_idx,
                                (word_idx + 1) % active_words,
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            );
                        }
                        self.used_pages.fetch_add(1, Ordering::Relaxed);
                        return Some(self.page_to_ptr(page_idx));
                    }

                    Err(actual) => word = actual, // Other thread accessed before us, retry
                }
            }
        }
        None
    }

    fn alloc_pages(&self, pages: usize) -> Option<*mut u8> {
        let total = self.total_pages.load(Ordering::Relaxed);
        let used = self.used_pages.load(Ordering::Relaxed);

        if pages > total || pages > total.saturating_sub(used) {
            return None;
        }

        let hint = self.search_hint.load(Ordering::Relaxed);

        if let Some(ptr) = self.try_allocate_pages(hint * 64, pages, total) {
            return Some(ptr);
        }

        if hint > 0 {
            return self.try_allocate_pages(0, pages, total);
        }

        None
    }

    fn try_allocate_pages(&self, from: usize, pages: usize, total: usize) -> Option<*mut u8> {
        let mut from = from;
        while let Some(start) = self.find_contiguous(from, pages, total) {
            if self.claim_range(start, pages) {
                self.used_pages.fetch_add(pages, Ordering::Relaxed);
                return Some(self.page_to_ptr(start));
            }

            from = start + 1;
        }
        None
    }

    /// Searches for `pages` consecutive free pages from `from`
    fn find_contiguous(&self, from: usize, pages: usize, total: usize) -> Option<usize> {
        let mut start = from;
        let mut taken = 0;

        let mut word_idx = from / 64;
        let mut bit = from % 64;
        let active_word = total.div_ceil(64);

        while word_idx < active_word {
            let word = self.bitmap[word_idx].load(Ordering::Relaxed);
            let limit = (total - word_idx * 64).min(64);

            if bit == 0 && word == 0 {
                if taken == 0 {
                    start = word_idx * 64;
                }
                taken += limit;
                if taken >= pages {
                    return Some(start);
                }
            } else if word == u64::MAX {
                taken = 0;
            } else {
                for offset in bit..limit {
                    if word & (1 << offset) == 0 {
                        if taken == 0 {
                            start = word_idx * 64 + offset;
                        }
                        taken += 1;
                        if taken >= pages {
                            return Some(start);
                        }
                    } else {
                        taken = 0;
                    }
                }
            }
            word_idx += 1;
            bit = 0;
        }
        None
    }

    /// Atomically claims `[start ... start + pages)`
    fn claim_range(&self, start: usize, pages: usize) -> bool {
        let mut word_idx = start / 64;
        let mut bit = start % 64;
        let mut remaning = pages;
        let mut claimed = 0;

        while claimed < pages {
            let take = remaning.min(64 - bit);
            let mask = (!0u64 >> (64 - take)) << bit;
            let atom = &self.bitmap[word_idx];
            let mut current = atom.load(Ordering::Relaxed);

            loop {
                if (current & mask) != 0 {
                    self.clear_range(start, claimed);
                    return false;
                }

                match atom.compare_exchange_weak(
                    current,
                    current | mask,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }

            claimed += take;
            remaning -= take;
            word_idx += 1;
            bit = 0;
        }
        true
    }

    /// Clears `[start ... start + pages)`
    fn clear_range(&self, start: usize, pages: usize) {
        let mut word_idx = start / 64;
        let mut bit = start % 64;
        let mut remaning = pages;

        while remaning > 0 {
            let take = remaning.min(64 - bit);
            let mask = (!0u64 >> (64 - take)) << bit;

            let _ = self.bitmap[word_idx].fetch_and(!mask, Ordering::Release);

            remaning -= take;
            word_idx += 1;
            bit = 0;
        }
    }

    pub fn dealloc(&self, ptr: *mut u8, pages: usize) {
        if pages == 0 {
            return;
        }

        let base = self.base_addr.load(Ordering::Relaxed);
        let addr = ptr as usize;
        assert!(addr >= base && (addr - base).is_multiple_of(PAGE_SIZE));

        let start_page = (addr - base) >> PAGE_ORDER;
        assert!(start_page + pages <= self.total_pages.load(Ordering::Relaxed));

        self.clear_range(start_page, pages);
        self.used_pages.fetch_sub(pages, Ordering::Relaxed);
        self.search_hint
            .fetch_min(start_page / 64, Ordering::Relaxed);
    }

    pub fn usage(&self) -> usize {
        self.used_pages.load(Ordering::Relaxed)
    }

    fn page_to_ptr(&self, idx: usize) -> *mut u8 {
        let base = self.base_addr.load(Ordering::Relaxed);
        ptr::without_provenance_mut(base + (idx << PAGE_ORDER))
    }
}
