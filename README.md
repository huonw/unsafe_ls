# unsafe_ls

List unsafe blocks and the unsafe actions within them.

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

    $ ./unsafe_ls -f test.rs  # only FFI
    test.rs:14:5: block with 1 ffi
            abort()

### All `unsafe`

    $ ./unsafe_ls -nf test.rs # all unsafe actions
    test.rs:3:1: fn with 1 static mut
        x += 1
    test.rs:7:5: block with 1 deref, 1 static mut
            *std::ptr::null::<int>();
            x += 1;
    test.rs:11:5: block with 1 unsafe call
            foo()
    test.rs:14:5: block with 1 ffi
            abort()


## Building

    rustc -O bin.rs

Known to work with Rust master at
6e7f170fedd3c526a643c0b2d13863acd982be02.

## Testimonials

I used it to submit
[#12445](https://github.com/mozilla/rust/pull/12445), reducing the
number of `transmute`s (since those are wildly unsafe) among other
small changes.
