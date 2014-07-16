# unsafe_ls

[![Build Status](https://travis-ci.org/huonw/unsafe_ls.png)](https://travis-ci.org/huonw/unsafe_ls)

List unsafe blocks and the unsafe actions within them, to enable
easier auditing of regions that need extra-careful examination. This
cannot catch memory-unsafe actions in safe code caused by bad `unsafe`
code, but correctly written/audited `unsafe` blocks will not cause
such problems.

It can be used to only display blocks that have non-FFI unsafety in
them, to avoid having to filter through lots of "routine" C calls.


Unfortunately [#11792](https://github.com/mozilla/rust/issues/11792)
means you may have to pass `-L` pointing to the directory that
contains the core crates (`std`, etc.) or edit the `DEFAULT_LIB_DIR`
static to avoiding the repetition.

## Examples

See `unsafe_ls -h` for all flags.

### All `unsafe` except for FFI

    $ ./unsafe_ls -n test.rs
    test.rs:3:1: fn with 1 static mut
        x += 1
    test.rs:7:5: block with 1 deref, 1 static mut
            *std::ptr::null::<int>();
            x += 1;
    test.rs:11:5: block with 1 unsafe call
            foo()

### Only FFI

    $ ./unsafe_ls -f test.rs
    test.rs:11:5: block with 1 ffi, 1 unsafe call
                abort()
    test.rs:17:5: block with 1 ffi
            abort()

### All `unsafe`

    $ ./unsafe_ls -nf test.rs
    test.rs:3:1: fn with 1 static mut
        x += 1
    test.rs:7:5: block with 1 deref, 1 static mut
            *std::ptr::null::<int>();
            x += 1;
    test.rs:11:5: block with 1 ffi, 1 unsafe call
            foo();
                abort()
    test.rs:17:5: block with 1 ffi
            abort()


## Building

    cargo build

Known to work with Rust master at 5cef16e.

## Testimonials

I used it to submit
[#12445](https://github.com/mozilla/rust/pull/12445), reducing the
number of `transmute`s (since those are wildly unsafe) among other
small changes.
