//! Compact, versioned runtime dataset encoding.

use crate::{HrirMeasurement, HrtfDataset, HrtfError, Vec3};

const MAGIC: &[u8; 8] = b"HRTFRT01";
const VERSION: u32 = 1;
const HEADER_LENGTH: usize = 24;
const DIRECTION_BYTES: usize = 3 * size_of::<f32>();

impl HrtfDataset {
    /// Serializes the validated dataset using explicit little-endian fields.
    ///
    /// # Errors
    ///
    /// Returns an error if a collection length cannot be represented by the version 1 format.
    pub fn to_runtime_bytes(&self) -> Result<Vec<u8>, HrtfError> {
        let measurement_bytes = DIRECTION_BYTES + 2 * self.hrir_length() * size_of::<f32>();
        let mut bytes = Vec::with_capacity(
            HEADER_LENGTH + self.measurements().len().saturating_mul(measurement_bytes),
        );
        bytes.extend_from_slice(MAGIC);
        push_u32(&mut bytes, VERSION);
        push_u32(&mut bytes, self.sample_rate());
        push_u32(
            &mut bytes,
            u32::try_from(self.hrir_length())
                .map_err(|_| HrtfError::InvalidDatasetBytes("HRIR length exceeds format limit"))?,
        );
        push_u32(
            &mut bytes,
            u32::try_from(self.measurements().len()).map_err(|_| {
                HrtfError::InvalidDatasetBytes("measurement count exceeds format limit")
            })?,
        );

        for measurement in self.measurements() {
            for component in measurement.direction.to_array() {
                push_f32(&mut bytes, component);
            }
            for &sample in measurement.left.iter().chain(&measurement.right) {
                push_f32(&mut bytes, sample);
            }
        }
        Ok(bytes)
    }

    /// Parses and validates the versioned little-endian runtime representation.
    ///
    /// # Errors
    ///
    /// Returns an error if the header, dimensions, direction vectors, samples, or total byte
    /// length are invalid.
    pub fn from_runtime_bytes(bytes: &[u8]) -> Result<Self, HrtfError> {
        if bytes.len() < HEADER_LENGTH {
            return Err(HrtfError::InvalidDatasetBytes("truncated header"));
        }
        if &bytes[..MAGIC.len()] != MAGIC {
            return Err(HrtfError::InvalidDatasetBytes("incorrect magic"));
        }

        let mut cursor = MAGIC.len();
        if read_u32(bytes, &mut cursor)? != VERSION {
            return Err(HrtfError::InvalidDatasetBytes("unsupported version"));
        }
        let sample_rate = read_u32(bytes, &mut cursor)?;
        let hrir_length = usize::try_from(read_u32(bytes, &mut cursor)?)
            .map_err(|_| HrtfError::InvalidDatasetBytes("HRIR length is too large"))?;
        let measurement_count = usize::try_from(read_u32(bytes, &mut cursor)?)
            .map_err(|_| HrtfError::InvalidDatasetBytes("measurement count is too large"))?;
        if hrir_length == 0 || measurement_count == 0 {
            return Err(HrtfError::InvalidDatasetBytes("zero-sized dimensions"));
        }

        let sample_bytes = 2_usize
            .checked_mul(hrir_length)
            .and_then(|count| count.checked_mul(size_of::<f32>()))
            .ok_or(HrtfError::InvalidDatasetBytes("dimensions overflow"))?;
        let measurement_bytes = DIRECTION_BYTES
            .checked_add(sample_bytes)
            .ok_or(HrtfError::InvalidDatasetBytes("dimensions overflow"))?;
        let expected_length = measurement_count
            .checked_mul(measurement_bytes)
            .and_then(|length| length.checked_add(HEADER_LENGTH))
            .ok_or(HrtfError::InvalidDatasetBytes("dimensions overflow"))?;
        if bytes.len() != expected_length {
            return Err(HrtfError::InvalidDatasetBytes("unexpected byte length"));
        }

        let mut measurements = Vec::with_capacity(measurement_count);
        for _ in 0..measurement_count {
            let direction = Vec3::new(
                read_f32(bytes, &mut cursor)?,
                read_f32(bytes, &mut cursor)?,
                read_f32(bytes, &mut cursor)?,
            );
            let left = read_samples(bytes, &mut cursor, hrir_length)?;
            let right = read_samples(bytes, &mut cursor, hrir_length)?;
            measurements.push(HrirMeasurement::new(direction, left, right)?);
        }
        Self::new(sample_rate, measurements)
    }
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, HrtfError> {
    let value = bytes
        .get(*cursor..*cursor + size_of::<u32>())
        .ok_or(HrtfError::InvalidDatasetBytes("truncated field"))?;
    *cursor += size_of::<u32>();
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| {
        HrtfError::InvalidDatasetBytes("invalid u32 field")
    })?))
}

fn read_f32(bytes: &[u8], cursor: &mut usize) -> Result<f32, HrtfError> {
    let bits = read_u32(bytes, cursor)?;
    Ok(f32::from_bits(bits))
}

fn read_samples(bytes: &[u8], cursor: &mut usize, count: usize) -> Result<Vec<f32>, HrtfError> {
    (0..count).map(|_| read_f32(bytes, cursor)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dataset() -> HrtfDataset {
        HrtfDataset::new(
            44_100,
            vec![
                HrirMeasurement::new(Vec3::Z, vec![0.25, -0.5], vec![0.75, 0.0]).unwrap(),
                HrirMeasurement::new(Vec3::X, vec![0.1, 0.2], vec![0.3, 0.4]).unwrap(),
                HrirMeasurement::new(Vec3::Y, vec![0.5, 0.6], vec![0.7, 0.8]).unwrap(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn runtime_format_round_trips_exactly() {
        let expected = dataset();
        let actual =
            HrtfDataset::from_runtime_bytes(&expected.to_runtime_bytes().unwrap()).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn runtime_format_rejects_wrong_magic_version_and_length() {
        let valid = dataset().to_runtime_bytes().unwrap();

        let mut wrong_magic = valid.clone();
        wrong_magic[0] = b'X';
        assert!(matches!(
            HrtfDataset::from_runtime_bytes(&wrong_magic),
            Err(HrtfError::InvalidDatasetBytes("incorrect magic"))
        ));

        let mut wrong_version = valid.clone();
        wrong_version[8..12].copy_from_slice(&2_u32.to_le_bytes());
        assert!(matches!(
            HrtfDataset::from_runtime_bytes(&wrong_version),
            Err(HrtfError::InvalidDatasetBytes("unsupported version"))
        ));

        assert!(matches!(
            HrtfDataset::from_runtime_bytes(&valid[..valid.len() - 1]),
            Err(HrtfError::InvalidDatasetBytes("unexpected byte length"))
        ));
    }

    #[test]
    fn packaged_mit_kemar_dataset_loads_with_expected_geometry() {
        let bytes = include_bytes!("../../../frontend/assets/mit-kemar.bhrtf");
        let dataset = HrtfDataset::from_runtime_bytes(bytes).unwrap();
        assert_eq!(dataset.sample_rate(), 44_100);
        assert_eq!(dataset.hrir_length(), 128);
        assert_eq!(dataset.measurements().len(), 710);

        let nearest = |target: Vec3| {
            dataset
                .measurements()
                .iter()
                .min_by(|left, right| {
                    left.direction
                        .angle_between(target)
                        .total_cmp(&right.direction.angle_between(target))
                })
                .unwrap()
        };
        let front = nearest(Vec3::Z);
        let right = nearest(Vec3::X);
        let left = nearest(Vec3::NEG_X);
        let up = nearest(Vec3::Y);
        for (measurement, expected) in [
            (front, Vec3::Z),
            (right, Vec3::X),
            (left, Vec3::NEG_X),
            (up, Vec3::Y),
        ] {
            assert!(measurement.direction.abs_diff_eq(expected, 1.0e-5));
        }

        assert_eq!(right.left, left.right);
        assert_eq!(right.right, left.left);
        let energy = |samples: &[f32]| samples.iter().map(|sample| sample * sample).sum::<f32>();
        assert!(energy(&right.right) > energy(&right.left));
    }
}
