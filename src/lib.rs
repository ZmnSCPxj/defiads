//! biadne

#![deny(non_upper_case_globals)]
#![deny(non_camel_case_types)]
#![deny(non_snake_case)]
#![deny(unused_mut)]
#![deny(missing_docs)]
#![deny(unused_must_use)]
#![forbid(unsafe_code)]


extern crate snap;
extern crate byteorder;
extern crate serde;
#[macro_use]
extern crate serde_derive;
#[cfg(test)]
extern crate hex;
extern crate bitcoin_hashes;
extern crate bitcoin;
extern crate siphasher;
extern crate secp256k1;
extern crate rand;

mod text;
mod ad;
mod iblt;
mod messages;
mod content;
mod funding;