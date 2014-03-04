extern { fn abort() -> !; }
static mut x: uint = 1;
unsafe fn foo() {
    x += 1
}
fn main() {
    unsafe {
        *std::ptr::null::<int>();
        x += 1;
    }
    unsafe {
        foo()
    }
    unsafe {
        abort()
    }
}
