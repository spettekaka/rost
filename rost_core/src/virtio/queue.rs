pub struct VirtQueue<'a> {
    queue_size: u32,
    head: u32,
    tail: u32,
    wrap: bool,
}
