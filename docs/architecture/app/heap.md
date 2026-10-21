# `app/heap.rs`

`heap.rs` — process-wide heap accounting. `Tracking` is installed as the global allocator and
adds only two atomics — **live** bytes (allocated − freed) and the running peak — over the
system allocator. It exists to answer one question the OS can't: whether memory growth is a
real leak or benign allocator/OS retention. Live returning to its baseline after a table closes
while the working set stays high is the allocator holding freed pages for reuse; live *not*
returning is the leak.
