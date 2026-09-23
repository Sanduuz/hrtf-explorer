//! Platform-independent spatial math and binaural DSP primitives.
//!
//! The canonical coordinate system is right-handed: +X is right, +Y is up,
//! and +Z is front. Azimuth is zero at the front and increases toward the
//! right; elevation increases upward.

pub mod binary;
pub mod convolution;
pub mod coordinates;
pub mod dataset;
pub mod interpolation;

pub use convolution::{
    BinauralOutput, Convolver, RealtimeBinauralConvolver, TimeDomainConvolver, render_binaural,
};
pub use coordinates::{
    SphericalDirection, angular_distance, direction_to_spherical, spherical_to_direction,
};
pub use dataset::{HrirMeasurement, HrtfDataset};
pub use glam::Vec3;
pub use interpolation::{
    HrirInterpolator, InterpolatedHrir, InterpolationContributor, NearestNeighborInterpolator,
    NearestThreeInterpolator, SphericalTriangleInterpolator, TimeAlignedNearestThreeInterpolator,
    TimeAlignedSphericalTriangleInterpolator,
};

use std::{error::Error, fmt};

/// Errors produced by dataset validation, spatial lookup, and DSP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HrtfError {
    EmptyDataset,
    EmptySignal,
    InvalidDirection,
    InvalidHrirLength,
    InvalidSignalLength,
    InvalidSampleRate,
    InvalidDatasetBytes(&'static str),
    NotEnoughMeasurements { available: usize, required: usize },
    NoContainingTriangle,
    NonFiniteSample,
}

impl fmt::Display for HrtfError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyDataset => write!(formatter, "the HRTF dataset has no measurements"),
            Self::EmptySignal => write!(formatter, "the input signal is empty"),
            Self::InvalidDirection => write!(formatter, "direction must be finite and non-zero"),
            Self::InvalidHrirLength => {
                write!(formatter, "all HRIR channels must have the declared length")
            }
            Self::InvalidSignalLength => {
                write!(
                    formatter,
                    "real-time input and output block lengths must match"
                )
            }
            Self::InvalidSampleRate => write!(formatter, "sample rate must be greater than zero"),
            Self::InvalidDatasetBytes(reason) => {
                write!(formatter, "invalid runtime HRTF dataset: {reason}")
            }
            Self::NotEnoughMeasurements {
                available,
                required,
            } => write!(
                formatter,
                "not enough HRTF measurements: found {available}, need at least {required}"
            ),
            Self::NoContainingTriangle => {
                write!(
                    formatter,
                    "no containing spherical measurement triangle was found"
                )
            }
            Self::NonFiniteSample => write!(formatter, "audio and HRIR samples must be finite"),
        }
    }
}

impl Error for HrtfError {}
