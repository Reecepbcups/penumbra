#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg_attr(docsrs, doc(cfg(feature = "component")))]
#[cfg(feature = "component")]
pub mod component;

pub mod event;
pub mod state_key;

mod action;
mod plan;
mod view;

mod lp;
mod proofs;
mod swap;
mod swap_claim;
