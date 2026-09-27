use glam::Vec3;

const MAGIC: &[u8; 8] = b"HMESH001";
const HEADER_LENGTH: usize = 12;
const VALUES_PER_VERTEX: usize = 6;
const HEAD_MESH: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/head-mesh.bin"));

#[derive(Clone, Copy, Debug)]
pub struct HeadVertex {
    pub position: Vec3,
    pub normal: Vec3,
}

pub fn vertices() -> Result<Vec<HeadVertex>, &'static str> {
    parse(HEAD_MESH)
}

fn parse(bytes: &[u8]) -> Result<Vec<HeadVertex>, &'static str> {
    if bytes.len() < HEADER_LENGTH || bytes.get(..8) != Some(MAGIC) {
        return Err("bundled head mesh has an invalid header");
    }
    let vertex_count = u32::from_le_bytes(
        bytes[8..12]
            .try_into()
            .map_err(|_| "bundled head mesh has an invalid vertex count")?,
    ) as usize;
    let expected_length = HEADER_LENGTH
        .checked_add(
            vertex_count
                .checked_mul(VALUES_PER_VERTEX * size_of::<f32>())
                .ok_or("bundled head mesh dimensions overflow")?,
        )
        .ok_or("bundled head mesh dimensions overflow")?;
    if bytes.len() != expected_length || vertex_count == 0 || vertex_count % 3 != 0 {
        return Err("bundled head mesh has invalid dimensions");
    }

    let mut vertices = Vec::with_capacity(vertex_count);
    for encoded in bytes[HEADER_LENGTH..].chunks_exact(VALUES_PER_VERTEX * size_of::<f32>()) {
        let mut values = [0.0; VALUES_PER_VERTEX];
        for (value, chunk) in values
            .iter_mut()
            .zip(encoded.chunks_exact(size_of::<f32>()))
        {
            *value = f32::from_le_bytes(
                chunk
                    .try_into()
                    .map_err(|_| "bundled head mesh has a truncated vertex")?,
            );
        }
        let position = Vec3::from_array(values[..3].try_into().map_err(|_| "invalid position")?);
        let normal = Vec3::from_array(values[3..].try_into().map_err(|_| "invalid normal")?);
        if !position.is_finite() || !normal.is_finite() || !normal.is_normalized() {
            return Err("bundled head mesh contains invalid vertex data");
        }
        vertices.push(HeadVertex { position, normal });
    }
    Ok(vertices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_head_is_valid_ear_aligned_triangle_geometry() {
        let vertices = vertices().unwrap();
        assert!(vertices.len() > 30_000);
        assert_eq!(vertices.len() % 3, 0);

        let minimum = vertices
            .iter()
            .map(|vertex| vertex.position)
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let maximum = vertices
            .iter()
            .map(|vertex| vertex.position)
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        assert!((maximum.y - minimum.y - 1.25).abs() < 1.0e-5);
        assert!((maximum.x + minimum.x).abs() < 1.0e-5);
        assert!((maximum.z + minimum.z).abs() < 1.0e-5);
        assert!(maximum.x < 0.5, "the wide shoulder region must be trimmed");
        let upper_head_start = f32::midpoint(minimum.y, maximum.y);
        let lateral_radius = vertices
            .iter()
            .filter(|vertex| vertex.position.y >= upper_head_start)
            .map(|vertex| vertex.position.x.abs())
            .reduce(f32::max)
            .unwrap();
        let ear_vertices = vertices
            .iter()
            .filter(|vertex| {
                vertex.position.y >= upper_head_start
                    && vertex.position.x.abs() >= lateral_radius * 0.86
            })
            .collect::<Vec<_>>();
        assert!(!ear_vertices.is_empty());
        let ear_count = u16::try_from(ear_vertices.len()).unwrap();
        let ear_axis_y = ear_vertices
            .iter()
            .map(|vertex| vertex.position.y)
            .sum::<f32>()
            / f32::from(ear_count);
        assert!((ear_axis_y - 0.12).abs() < 1.0e-3);
        assert!(maximum.z > 0.3, "the face must point toward canonical +Z");
        assert!(vertices.iter().all(|vertex| vertex.normal.is_normalized()));
    }
}
