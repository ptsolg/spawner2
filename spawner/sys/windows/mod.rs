#[allow(clippy::cast_ptr_alignment)]
mod helpers;

#[allow(non_camel_case_types, non_snake_case, non_upper_case_globals)]
mod missing_decls;

pub mod error;
pub mod pipe;
pub mod pipe_ext;
pub mod process;
pub mod process_ext;
