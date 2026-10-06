use std::{ops::Deref, sync::atomic::{AtomicI64, AtomicPtr, AtomicU64, Ordering}};

use poniescript_gc::HasPsType;

use crate::*;
