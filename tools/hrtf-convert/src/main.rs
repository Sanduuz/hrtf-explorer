use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

use hrtf::{HrirMeasurement, HrtfDataset, spherical_to_direction};

const EXPECTED_SAMPLE_RATE: u32 = 44_100;
const EXPECTED_HRIR_LENGTH: usize = 128;
const EXPECTED_SOURCE_FILES: usize = 368;
const EXPECTED_RUNTIME_MEASUREMENTS: usize = 710;

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let (input, output, limit) = parse_arguments(&arguments)?;
    let dataset = convert_directory(input, limit)?;
    let bytes = dataset.to_runtime_bytes()?;
    let verified = HrtfDataset::from_runtime_bytes(&bytes)?;
    if verified.sample_rate() != dataset.sample_rate()
        || verified.hrir_length() != dataset.hrir_length()
        || verified.measurements().len() != dataset.measurements().len()
    {
        return Err("runtime dataset failed serialization round-trip validation".into());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, &bytes)?;
    println!(
        "converted {} measurements at {} Hz ({} samples/channel) into {} bytes",
        dataset.measurements().len(),
        dataset.sample_rate(),
        dataset.hrir_length(),
        bytes.len()
    );
    Ok(())
}

fn parse_arguments(arguments: &[String]) -> Result<(&Path, &Path, Option<usize>), Box<dyn Error>> {
    match arguments {
        [input, output] => Ok((Path::new(input), Path::new(output), None)),
        [input, output, flag, value] if flag == "--limit" => {
            let limit = value.parse::<usize>()?;
            if limit == 0 {
                return Err("--limit must be greater than zero".into());
            }
            Ok((Path::new(input), Path::new(output), Some(limit)))
        }
        _ => Err(
            "usage: hrtf-convert <extracted-compact-directory> <output.bhrtf> [--limit N]".into(),
        ),
    }
}

fn convert_directory(
    directory: &Path,
    limit: Option<usize>,
) -> Result<HrtfDataset, Box<dyn Error>> {
    let mut sources = collect_wav_sources(directory)?;
    sources.sort_by_key(|source| (source.elevation_degrees, source.azimuth_degrees));
    if sources.windows(2).any(|pair| {
        pair[0].elevation_degrees == pair[1].elevation_degrees
            && pair[0].azimuth_degrees == pair[1].azimuth_degrees
    }) {
        return Err("compact KEMAR input contains a duplicate measurement direction".into());
    }
    if let Some(limit) = limit {
        sources.truncate(limit);
    } else if sources.len() != EXPECTED_SOURCE_FILES {
        return Err(format!(
            "complete compact KEMAR input must contain {EXPECTED_SOURCE_FILES} WAV files; found {}",
            sources.len()
        )
        .into());
    }

    let mut measurements = Vec::with_capacity(sources.len() * 2);
    for source in sources {
        let audio = read_compact_wav(&source.path)?;
        let direction = spherical_to_direction(
            f32::from(source.azimuth_degrees),
            f32::from(source.elevation_degrees),
        );
        measurements.push(HrirMeasurement::new(
            direction,
            audio.left.clone(),
            audio.right.clone(),
        )?);

        if source.azimuth_degrees != 0 && source.azimuth_degrees != 180 {
            let mirrored_azimuth = 360_u16 - source.azimuth_degrees;
            measurements.push(HrirMeasurement::new(
                spherical_to_direction(
                    f32::from(mirrored_azimuth),
                    f32::from(source.elevation_degrees),
                ),
                audio.right,
                audio.left,
            )?);
        }
    }
    let dataset = HrtfDataset::new(EXPECTED_SAMPLE_RATE, measurements)?;
    if limit.is_none() && dataset.measurements().len() != EXPECTED_RUNTIME_MEASUREMENTS {
        return Err(format!(
            "mirrored runtime dataset must contain {EXPECTED_RUNTIME_MEASUREMENTS} measurements; found {}",
            dataset.measurements().len()
        )
        .into());
    }
    Ok(dataset)
}

#[derive(Debug)]
struct SourceFile {
    path: PathBuf,
    elevation_degrees: i16,
    azimuth_degrees: u16,
}

fn collect_wav_sources(directory: &Path) -> Result<Vec<SourceFile>, Box<dyn Error>> {
    let mut pending = vec![directory.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|extension| extension.to_str()) == Some("wav") {
                let (elevation_degrees, azimuth_degrees) = parse_source_angles(&path)?;
                sources.push(SourceFile {
                    path,
                    elevation_degrees,
                    azimuth_degrees,
                });
            }
        }
    }
    if sources.is_empty() {
        return Err(format!(
            "no compact KEMAR WAV files found under {}",
            directory.display()
        )
        .into());
    }
    Ok(sources)
}

fn parse_source_angles(path: &Path) -> Result<(i16, u16), Box<dyn Error>> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("source filename is not valid UTF-8")?;
    let stem = filename
        .strip_prefix('H')
        .and_then(|name| name.strip_suffix("a.wav"))
        .ok_or_else(|| format!("unexpected compact KEMAR filename: {filename}"))?;
    let (elevation, azimuth) = stem
        .split_once('e')
        .ok_or_else(|| format!("missing angle separator in filename: {filename}"))?;
    let elevation = elevation.parse::<i16>()?;
    let azimuth = azimuth.parse::<u16>()?;
    if !(-40..=90).contains(&elevation) || !(0..=180).contains(&azimuth) {
        return Err(format!("angles outside compact KEMAR range in {filename}").into());
    }
    Ok((elevation, azimuth))
}

#[derive(Debug, PartialEq)]
struct StereoHrir {
    left: Vec<f32>,
    right: Vec<f32>,
}

fn read_compact_wav(path: &Path) -> Result<StereoHrir, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(format!("{} is not a RIFF/WAVE file", path.display()).into());
    }

    let mut cursor = 12_usize;
    let mut format = None;
    let mut data = None;
    while cursor + 8 <= bytes.len() {
        let chunk_id = &bytes[cursor..cursor + 4];
        let chunk_length = usize::try_from(read_u32_le(&bytes, cursor + 4)?)?;
        let start = cursor + 8;
        let end = start
            .checked_add(chunk_length)
            .ok_or("WAV chunk length overflow")?;
        let chunk = bytes
            .get(start..end)
            .ok_or_else(|| format!("truncated WAV chunk in {}", path.display()))?;
        match chunk_id {
            b"fmt " => format = Some(parse_format_chunk(chunk)?),
            b"data" => data = Some(chunk),
            _ => {}
        }
        cursor = end + (chunk_length % 2);
    }

    let format = format.ok_or("WAV file has no format chunk")?;
    if format != (1, 2, EXPECTED_SAMPLE_RATE, 16) {
        return Err(format!("unsupported WAV format in {}: {format:?}", path.display()).into());
    }
    let data = data.ok_or("WAV file has no data chunk")?;
    let expected_bytes = EXPECTED_HRIR_LENGTH * 2 * size_of::<i16>();
    if data.len() != expected_bytes {
        return Err(format!(
            "{} has {} PCM bytes; expected {expected_bytes}",
            path.display(),
            data.len()
        )
        .into());
    }

    let mut left = Vec::with_capacity(EXPECTED_HRIR_LENGTH);
    let mut right = Vec::with_capacity(EXPECTED_HRIR_LENGTH);
    for frame in data.chunks_exact(4) {
        left.push(f32::from(i16::from_le_bytes([frame[0], frame[1]])) / 32_768.0);
        right.push(f32::from(i16::from_le_bytes([frame[2], frame[3]])) / 32_768.0);
    }
    Ok(StereoHrir { left, right })
}

fn parse_format_chunk(chunk: &[u8]) -> Result<(u16, u16, u32, u16), Box<dyn Error>> {
    if chunk.len() < 16 {
        return Err("truncated WAV format chunk".into());
    }
    Ok((
        read_u16_le(chunk, 0)?,
        read_u16_le(chunk, 2)?,
        read_u32_le(chunk, 4)?,
        read_u16_le(chunk, 14)?,
    ))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, Box<dyn Error>> {
    let field: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or("truncated little-endian u16")?
        .try_into()?;
    Ok(u16::from_le_bytes(field))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, Box<dyn Error>> {
    let field: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or("truncated little-endian u32")?
        .try_into()?;
    Ok(u32::from_le_bytes(field))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_positive_and_negative_source_angles() {
        assert_eq!(
            parse_source_angles(Path::new("H-40e006a.wav")).unwrap(),
            (-40, 6)
        );
        assert_eq!(
            parse_source_angles(Path::new("H90e000a.wav")).unwrap(),
            (90, 0)
        );
    }

    #[test]
    fn rejects_out_of_range_or_unexpected_names() {
        assert!(parse_source_angles(Path::new("H0e181a.wav")).is_err());
        assert!(parse_source_angles(Path::new("left.wav")).is_err());
    }

    #[test]
    fn parses_minimal_pcm_wav_and_preserves_channel_order() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36_u32 + 512).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&EXPECTED_SAMPLE_RATE.to_le_bytes());
        bytes.extend_from_slice(&(EXPECTED_SAMPLE_RATE * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&512_u32.to_le_bytes());
        for _ in 0..EXPECTED_HRIR_LENGTH {
            bytes.extend_from_slice(&16_384_i16.to_le_bytes());
            bytes.extend_from_slice(&(-8_192_i16).to_le_bytes());
        }

        let temporary = std::env::temp_dir().join(format!(
            "hrtf-convert-test-{}-{}.wav",
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed")
        ));
        fs::write(&temporary, bytes).unwrap();
        let parsed = read_compact_wav(&temporary).unwrap();
        fs::remove_file(temporary).unwrap();
        assert_eq!(parsed.left, vec![0.5; EXPECTED_HRIR_LENGTH]);
        assert_eq!(parsed.right, vec![-0.25; EXPECTED_HRIR_LENGTH]);
    }
}
