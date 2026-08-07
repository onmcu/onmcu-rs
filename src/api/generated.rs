//! This file lives in OUT_DIR and is produced by build.rs
#![allow(clippy::all, clippy::pedantic, clippy::nursery)]
include!(concat!(env!("OUT_DIR"), "/codegen.rs"));
