extern { fn abort() -> !; }
static mut x: i32 = 1;
unsafe fn foo() {
    x += 1
}
fn bar() {
    unsafe {
        std::mem::transmute::<&i32, &mut i32>(&1);
        let _ = 0 as *const i32 as *mut i32;
    }
}
fn main() {
    unsafe {
        *std::ptr::null::<i32>();
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
