extern { fn abort() -> !; }
static mut x: uint = 1;
unsafe fn foo() {
    x += 1
}
fn bar() {
    unsafe {
        std::mem::transmute::<&int, &mut int>(&1);
        let _ = 0 as *const  int as *mut int;
    }
}
fn main() {
    unsafe {
        *std::ptr::null::<int>();
        x += 1;
    }
    unsafe {
        foo();
        if false {
            abort()
        }
    }
    bar();
    unsafe {
        abort()
    }
}
