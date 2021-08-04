/*
Copyright (c) 2021 VMware, Inc.
SPDX-License-Identifier: MIT
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:
The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.
THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

/* `hmap` functions from OVS. Because these are `inline`, bindgen does not generate them. */

extern crate ovsdb_sys;

use std::{
    convert::TryInto,
    ptr,
};


pub fn shash(
    hmap_node: *const ovsdb_sys::hmap_node
) -> *const ovsdb_sys::shash_node {
    hmap_node
        .cast::<u8>()
        .wrapping_sub(offset_of!(ovsdb_sys::shash_node, node))
        .cast::<ovsdb_sys::shash_node>()
}

/* Returns the first node in 'hmap', in arbitrary order.
 * If 'hmap' is empty, return a null pointer. */
pub fn first(
    hmap: *const ovsdb_sys::hmap
) -> *const ovsdb_sys::hmap_node {
    return hmap_next__(hmap, 0);
}

/* Returns the next node in 'hmap' following node, in arbitrary order.
 * If 'node' is the last node in 'hmap', it returns null.
 *
 * If the hash map has been reallocated since 'node' was visited,
 * some nodes may be skipped or visited twice.
 * (Removing 'node' from the hash map does not prevent calling this function,
 * since node->next is preserved. Freeing 'node' does prevent calling it.) */
pub fn next(
    hmap: *const ovsdb_sys::hmap,
    node: *const ovsdb_sys::hmap_node,
) -> *const ovsdb_sys::hmap_node {
    if node.is_null() || hmap.is_null() {
        return node;
    }

    /* We checked both pointers for null, so below dereferences are safe. */
    let next = unsafe {(*node).next};
    if !next.is_null() {
        return next;
    }

    let start = 1 + unsafe{(*node).hash & (*hmap).mask};
    hmap_next__(hmap, start)
}

fn hmap_next__(
    hmap: *const ovsdb_sys::hmap,
    start: ovsdb_sys::size_t,
) -> *const ovsdb_sys::hmap_node {
    if hmap.is_null() {
        return ptr::null();
    }

    /* The initial null check makes this dereference safe. */
    let mask: ovsdb_sys::size_t = unsafe{(*hmap).mask};

    let mut i: ovsdb_sys::size_t = start;
    while i <= mask {
        let idx = i.try_into().unwrap();

        /* Both dereferenced pointers are checked for null. */
        unsafe {
            let node = (*hmap).buckets.offset(idx);
            if !node.is_null() {
                return (*node);
            }
        }
        
        i += 1;
    }

    ptr::null()
}
