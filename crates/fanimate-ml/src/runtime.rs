//! The runtime behind a small trait.
//!
//! Decision D2-5 chose ONNX Runtime as an optional download, behind a trait
//! so the choice can change. **No runtime is bundled yet**: the base
//! installer does not grow, and nothing here pretends to run a model. A tool
//! asks [`find`] for the runtime and, with none installed, says that it
//! needs one (`ModelError::NoRuntime`) instead of failing in a strange way.
//! When a runtime is added it implements [`Runtime`] in its own module and
//! `find` returns it.

use crate::package::ModelError;
use crate::store::Loaded;

/// A tensor of 32-bit floats: the shape and the values, row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Tensor {
    pub shape: Vec<usize>,
    pub data: Vec<f32>,
}

impl Tensor {
    /// A tensor whose data fit its shape, within a size a model could ask for.
    pub fn new(shape: Vec<usize>, data: Vec<f32>) -> Result<Self, String> {
        let count = shape.iter().try_fold(1usize, |n, d| n.checked_mul(*d));
        match count {
            Some(n) if n == data.len() && n <= 1 << 28 => Ok(Self { shape, data }),
            _ => Err("the tensor's data do not fit its shape".to_owned()),
        }
    }
}

/// A model ready to run.
pub trait Session: Send {
    /// Run the model on `inputs` and return its outputs.
    fn run(&mut self, inputs: &[Tensor]) -> Result<Vec<Tensor>, ModelError>;
}

/// Something that can run weights of one kind (`onnx`).
pub trait Runtime: Send + Sync {
    /// The runtime's name, as a manifest's `runtime` says it.
    fn name(&self) -> &'static str;

    /// Whether it can run models on this computer.
    fn available(&self) -> bool;

    /// Open `model` for running.
    fn open(&self, model: &Loaded) -> Result<Box<dyn Session>, ModelError>;
}

/// The runtime used when none is installed: always unavailable.
pub struct NoRuntime;

impl Runtime for NoRuntime {
    fn name(&self) -> &'static str {
        "none"
    }

    fn available(&self) -> bool {
        false
    }

    fn open(&self, model: &Loaded) -> Result<Box<dyn Session>, ModelError> {
        Err(ModelError::NoRuntime(format!("\"{}\"", model.manifest.name)))
    }
}

/// The runtime for weights of kind `kind` (a manifest's `runtime`), or
/// [`NoRuntime`] when the computer has none.
pub fn find(_kind: &str) -> Box<dyn Runtime> {
    Box::new(NoRuntime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::sound_manifest;

    #[test]
    fn with_no_runtime_a_model_says_so_instead_of_failing_oddly() {
        let rt = find("onnx");
        assert!(!rt.available());
        let loaded = Loaded {
            manifest: sound_manifest(),
            weights: vec![1, 2, 3],
        };
        let err = rt.open(&loaded).err().unwrap();
        assert!(err.to_string().contains("no model runtime"), "{err}");
    }

    #[test]
    fn a_tensor_must_fit_its_shape_and_stay_a_sane_size() {
        assert!(Tensor::new(vec![2, 3], vec![0.0; 6]).is_ok());
        assert!(Tensor::new(vec![2, 3], vec![0.0; 5]).is_err());
        assert!(Tensor::new(vec![usize::MAX, 2], vec![]).is_err());
        assert!(Tensor::new(vec![1 << 20, 1 << 20], vec![]).is_err());
        assert!(Tensor::new(vec![], vec![1.0]).is_ok(), "a scalar");
    }
}
