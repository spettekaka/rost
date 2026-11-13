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

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::AtomicU8;
    use std::boxed::Box;
    use std::vec::Vec;

    const TEST_BASE: usize = 0x1000_0000;

    fn make_allocator(base: usize, end: usize) -> Box<AtomicPageAllocator> {
        let alloc = Box::new(AtomicPageAllocator::new());
        alloc.init(base, end);
        alloc
    }

    fn make_allocator_pages(pages: usize) -> Box<AtomicPageAllocator> {
        make_allocator(TEST_BASE, TEST_BASE + pages * PAGE_SIZE)
    }

    fn page_index(base: usize, ptr: *mut u8) -> usize {
        let addr = ptr.addr();
        assert!(addr >= base, "pointer {addr:#x} below base {base:#x}");
        assert!(
            (addr - base).is_multiple_of(PAGE_SIZE),
            "pointer {addr:#x} not page-aligned relative to {base:#x}"
        );
        (addr - base) >> PAGE_ORDER
    }

    fn check_and_claim(owned: &[AtomicU8], base: usize, ptr: *mut u8, pages: usize) {
        let start = page_index(base, ptr);
        assert!(start + pages <= owned.len());
        for slot in &owned[start..start + pages] {
            assert_eq!(
                slot.swap(1, Ordering::AcqRel),
                0,
                "overlapping page allocation detected"
            );
        }
    }

    fn release_and_dealloc(
        alloc: &AtomicPageAllocator,
        owned: &[AtomicU8],
        base: usize,
        ptr: *mut u8,
        pages: usize,
    ) {
        let start = page_index(base, ptr);
        assert!(start + pages <= owned.len());
        for slot in &owned[start..start + pages] {
            assert_eq!(slot.swap(0, Ordering::Release), 1);
        }
        alloc.dealloc(ptr, pages);
    }

    #[test]
    fn test_new_and_default_uninitialized() {
        let alloc_new = Box::new(AtomicPageAllocator::new());
        let alloc_def = Box::<AtomicPageAllocator>::default();

        for alloc in [&alloc_new, &alloc_def] {
            assert_eq!(alloc.usage(), 0);
            assert!(alloc.alloc(0).is_none());
            assert!(alloc.alloc(1).is_none());
            assert!(alloc.alloc(4).is_none());
            alloc.dealloc(ptr::null_mut(), 0);
            assert_eq!(alloc.usage(), 0);
        }
    }

    #[test]
    fn test_init_alignment_and_reinit() {
        // base_addr rounds up to TEST_BASE + PAGE_SIZE; end_addr rounds down to TEST_BASE + 3 * PAGE_SIZE.
        let alloc = make_allocator(TEST_BASE + 1, TEST_BASE + 3 * PAGE_SIZE + 500);
        let expected_base = TEST_BASE + PAGE_SIZE;

        assert_eq!(alloc.usage(), 0);
        let p0 = alloc.alloc(1).expect("first aligned page should allocate");
        let p1 = alloc.alloc(1).expect("second aligned page should allocate");
        assert_eq!(p0.addr(), expected_base);
        assert_eq!(p1.addr(), expected_base + PAGE_SIZE);
        assert_eq!(alloc.usage(), 2);
        assert!(alloc.alloc(1).is_none());

        // Re-initializing should reset usage, bitmap, and bounds.
        let new_base = TEST_BASE + 0x10_0000;
        alloc.init(new_base, new_base + 4 * PAGE_SIZE);
        assert_eq!(alloc.usage(), 0);

        let all = alloc.alloc(4).expect("re-initialized pool should be free");
        assert_eq!(all.addr(), new_base);
        assert_eq!(alloc.usage(), 4);
        assert!(alloc.alloc(1).is_none());
    }

    #[test]
    #[should_panic(expected = "Invalid page pool supplied")]
    fn test_init_equal_bounds_panics() {
        let alloc = Box::new(AtomicPageAllocator::new());
        alloc.init(TEST_BASE, TEST_BASE);
    }

    #[test]
    #[should_panic(expected = "Invalid page pool supplied")]
    fn test_init_inverted_bounds_panics() {
        let alloc = Box::new(AtomicPageAllocator::new());
        alloc.init(TEST_BASE + PAGE_SIZE, TEST_BASE);
    }

    #[test]
    #[should_panic(expected = "Invalid page pool supplied")]
    fn test_init_sub_page_range_panics() {
        let alloc = Box::new(AtomicPageAllocator::new());
        alloc.init(TEST_BASE + 1, TEST_BASE + 2 * PAGE_SIZE - 1);
    }

    #[test]
    fn test_init_clamps_to_max_pages() {
        let alloc = make_allocator(TEST_BASE, TEST_BASE + (MAX_PAGES + 16) * PAGE_SIZE);

        let all = alloc
            .alloc(MAX_PAGES)
            .expect("MAX_PAGES contiguous allocation should succeed");
        assert_eq!(all.addr(), TEST_BASE);
        assert_eq!(alloc.usage(), MAX_PAGES);
        assert!(alloc.alloc(1).is_none());

        alloc.dealloc(all, MAX_PAGES);
        assert_eq!(alloc.usage(), 0);
        assert!(alloc.alloc(1).is_some());
    }

    #[test]
    fn test_alloc_zero_and_dealloc_zero() {
        let alloc = make_allocator_pages(4);

        assert!(alloc.alloc(0).is_none());
        assert_eq!(alloc.usage(), 0);

        let ptr = alloc
            .alloc(1)
            .expect("single page allocation should succeed");
        alloc.dealloc(ptr, 0);
        alloc.dealloc(ptr::null_mut(), 0);
        assert_eq!(alloc.usage(), 1);

        alloc.dealloc(ptr, 1);
        assert_eq!(alloc.usage(), 0);
    }

    #[test]
    fn test_single_page_exhaust_and_reuse() {
        const PAGES: usize = 10;
        let alloc = make_allocator_pages(PAGES);
        let mut ptrs = Vec::with_capacity(PAGES);

        for i in 0..PAGES {
            let ptr = alloc.alloc(1).expect("page allocation should succeed");
            assert_eq!(page_index(TEST_BASE, ptr), i);
            ptrs.push(ptr);
            assert_eq!(alloc.usage(), i + 1);
        }

        assert!(alloc.alloc(1).is_none());

        let freed = ptrs.remove(4);
        alloc.dealloc(freed, 1);
        assert_eq!(alloc.usage(), PAGES - 1);

        let reused = alloc.alloc(1).expect("freed page should be reusable");
        assert_eq!(reused.addr(), freed.addr());
        assert_eq!(alloc.usage(), PAGES);
        ptrs.push(reused);

        for ptr in ptrs {
            alloc.dealloc(ptr, 1);
        }
        assert_eq!(alloc.usage(), 0);
    }

    #[test]
    fn test_non_multiple_of_64_multiword_boundary() {
        const PAGES: usize = 64 + 5;
        let alloc = make_allocator_pages(PAGES);
        let mut ptrs = Vec::with_capacity(PAGES);

        for i in 0..PAGES {
            let ptr = alloc.alloc(1).expect("page allocation should succeed");
            assert_eq!(page_index(TEST_BASE, ptr), i);
            ptrs.push(ptr);
        }
        assert!(alloc.alloc(1).is_none());
        assert_eq!(alloc.usage(), PAGES);

        for ptr in ptrs {
            alloc.dealloc(ptr, 1);
        }
        assert_eq!(alloc.usage(), 0);
        assert!(alloc.alloc(PAGES + 1).is_none());

        let all = alloc.alloc(PAGES).expect("exact pool size should allocate");
        assert_eq!(all.addr(), TEST_BASE);
        alloc.dealloc(all, PAGES);
        assert_eq!(alloc.usage(), 0);
    }

    #[test]
    fn test_alloc_contiguous_across_word_boundaries() {
        const PAGES: usize = 192;
        let alloc = make_allocator_pages(PAGES);

        let head = alloc.alloc(60).expect("60-page allocation should succeed");
        assert_eq!(page_index(TEST_BASE, head), 0);

        // Spans word 0 (bits 60..64) and word 1 (bits 0..4).
        let cross = alloc
            .alloc(8)
            .expect("cross-word allocation should succeed");
        assert_eq!(page_index(TEST_BASE, cross), 60);

        // Spans > 64 pages across word 1 and word 2.
        let large = alloc
            .alloc(100)
            .expect("100-page allocation should succeed");
        assert_eq!(page_index(TEST_BASE, large), 68);
        assert_eq!(alloc.usage(), 168);

        assert!(alloc.alloc(25).is_none());
        let tail = alloc.alloc(24).expect("remaining 24 pages should allocate");
        assert_eq!(page_index(TEST_BASE, tail), 168);
        assert_eq!(alloc.usage(), PAGES);

        alloc.dealloc(head, 60);
        alloc.dealloc(cross, 8);
        alloc.dealloc(large, 100);
        alloc.dealloc(tail, 24);
        assert_eq!(alloc.usage(), 0);

        let whole = alloc.alloc(PAGES).expect("full pool should coalesce");
        assert_eq!(whole.addr(), TEST_BASE);
        alloc.dealloc(whole, PAGES);
    }

    #[test]
    fn test_fragmentation_and_coalescing() {
        let alloc = make_allocator_pages(8);
        let mut ptrs = Vec::with_capacity(8);
        for _ in 0..8 {
            ptrs.push(alloc.alloc(1).unwrap());
        }

        // Free non-adjacent pages so no 2-page contiguous run exists.
        for idx in [1, 3, 5, 7] {
            alloc.dealloc(ptrs[idx], 1);
        }
        assert_eq!(alloc.usage(), 4);
        assert!(alloc.alloc(2).is_none());

        // Freeing page 2 coalesces [1..4) into a 3-page run.
        alloc.dealloc(ptrs[2], 1);
        assert_eq!(alloc.usage(), 3);

        let coalesced = alloc
            .alloc(3)
            .expect("coalesced 3-page run should allocate");
        assert_eq!(page_index(TEST_BASE, coalesced), 1);
        assert_eq!(alloc.usage(), 6);

        alloc.dealloc(coalesced, 3);
        for idx in [0, 4, 6] {
            alloc.dealloc(ptrs[idx], 1);
        }
        assert_eq!(alloc.usage(), 0);
    }

    #[test]
    fn test_search_hint_advance_reset_and_fallback() {
        let alloc = make_allocator_pages(128);
        let mut word0 = Vec::with_capacity(64);
        for _ in 0..64 {
            word0.push(alloc.alloc(1).unwrap());
        }
        // Word 0 is full, so search_hint advanced to 1.
        let word1 = alloc.alloc(60).expect("word 1 should have 60 pages");
        assert_eq!(page_index(TEST_BASE, word1), 64);

        // Exercises alloc_pages with hint > 0 failing from hint*64 and falling back to 0.
        assert!(alloc.alloc(8).is_none());

        // Freeing pages in word 0 resets search_hint back to 0 via fetch_min.
        for ptr in word0.drain(..8) {
            alloc.dealloc(ptr, 1);
        }
        let reused = alloc.alloc(8).expect("freed run in word 0 should allocate");
        assert_eq!(page_index(TEST_BASE, reused), 0);

        alloc.dealloc(reused, 8);
        for ptr in word0 {
            alloc.dealloc(ptr, 1);
        }
        alloc.dealloc(word1, 60);
        assert_eq!(alloc.usage(), 0);
    }

    #[test]
    #[should_panic]
    fn test_dealloc_below_base_panics() {
        let alloc = make_allocator_pages(4);
        alloc.dealloc(ptr::without_provenance_mut(TEST_BASE - PAGE_SIZE), 1);
    }

    #[test]
    #[should_panic]
    fn test_dealloc_unaligned_panics() {
        let alloc = make_allocator_pages(4);
        alloc.dealloc(ptr::without_provenance_mut(TEST_BASE + 1), 1);
    }

    #[test]
    #[should_panic]
    fn test_dealloc_out_of_bounds_panics() {
        let alloc = make_allocator_pages(4);
        alloc.dealloc(ptr::without_provenance_mut(TEST_BASE + 3 * PAGE_SIZE), 2);
    }

    #[test]
    fn test_concurrent_single_page_alloc_dealloc() {
        const PAGES: usize = 128;
        const THREADS: usize = 4;
        const ROUNDS: usize = 32;

        let alloc = make_allocator_pages(PAGES);
        let owned: Vec<AtomicU8> = (0..PAGES).map(|_| AtomicU8::new(0)).collect();

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    let mut held = Vec::new();
                    for round in 0..ROUNDS {
                        for _ in 0..4 {
                            if let Some(ptr) = alloc.alloc(1) {
                                check_and_claim(&owned, TEST_BASE, ptr, 1);
                                held.push(ptr);
                            }
                        }
                        let drain_count = if round % 2 == 0 {
                            held.len() / 2
                        } else {
                            held.len()
                        };
                        for ptr in held.drain(..drain_count) {
                            release_and_dealloc(&alloc, &owned, TEST_BASE, ptr, 1);
                        }
                    }
                    for ptr in held {
                        release_and_dealloc(&alloc, &owned, TEST_BASE, ptr, 1);
                    }
                });
            }
        });

        assert_eq!(alloc.usage(), 0);
        assert!(owned.iter().all(|slot| slot.load(Ordering::Relaxed) == 0));
        let whole = alloc
            .alloc(PAGES)
            .expect("all pages should be available after concurrent test");
        alloc.dealloc(whole, PAGES);
    }

    #[test]
    fn test_concurrent_mixed_contiguous_alloc_dealloc() {
        const PAGES: usize = 512;
        const THREADS: usize = 4;
        const ROUNDS: usize = 16;

        let alloc = make_allocator_pages(PAGES);
        let owned: Vec<AtomicU8> = (0..PAGES).map(|_| AtomicU8::new(0)).collect();

        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    let mut held = Vec::new();
                    for _ in 0..ROUNDS {
                        for count in [1, 2, 5, 65] {
                            if let Some(ptr) = alloc.alloc(count) {
                                check_and_claim(&owned, TEST_BASE, ptr, count);
                                held.push((ptr, count));
                            }
                        }
                        for (ptr, count) in held.drain(..) {
                            release_and_dealloc(&alloc, &owned, TEST_BASE, ptr, count);
                        }
                    }
                });
            }
        });

        assert_eq!(alloc.usage(), 0);
        assert!(owned.iter().all(|slot| slot.load(Ordering::Relaxed) == 0));
        let whole = alloc
            .alloc(PAGES)
            .expect("all pages should be available after mixed concurrent test");
        alloc.dealloc(whole, PAGES);
    }
}
