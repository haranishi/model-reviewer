mod build;
pub mod error;
pub mod export;
mod inspect;

pub use error::{ExtensionKind, LoadError};
use model_reviewer_core::scene::Model;

pub fn load_glb(bytes: &[u8]) -> Result<Model, LoadError> {
    let opened = inspect::open(bytes)?;
    build::assemble(opened)
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_input_is_an_error() {
        assert!(crate::load_glb(&[]).is_err());
    }
}
