use std::{
    env, fs,
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"HMESH001";
const TARGET_HEIGHT: f32 = 1.25;

fn main() {
    let source = Path::new("assets/head-male.obj");
    println!("cargo:rerun-if-changed={}", source.display());

    let obj = fs::read_to_string(source).expect("failed to read bundled head OBJ");
    let mesh = convert_obj(&obj).expect("failed to preprocess bundled head OBJ");
    let output =
        PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is missing")).join("head-mesh.bin");
    fs::write(output, mesh).expect("failed to write preprocessed head mesh");
}

#[allow(clippy::too_many_lines)]
fn convert_obj(source: &str) -> Result<Vec<u8>, String> {
    let mut positions = Vec::<[f32; 3]>::new();
    let mut polygons = Vec::<Vec<usize>>::new();

    for (line_index, line) in source.lines().enumerate() {
        let mut fields = line.split_whitespace();
        match fields.next() {
            Some("v") => {
                let mut position = [0.0; 3];
                for component in &mut position {
                    *component = fields
                        .next()
                        .ok_or_else(|| format!("vertex on line {} is incomplete", line_index + 1))?
                        .parse::<f32>()
                        .map_err(|error| {
                            format!("invalid vertex on line {}: {error}", line_index + 1)
                        })?;
                }
                positions.push(position);
            }
            Some("f") => {
                let polygon = fields
                    .map(|field| {
                        let index = field
                            .split('/')
                            .next()
                            .ok_or_else(|| format!("invalid face on line {}", line_index + 1))?
                            .parse::<usize>()
                            .map_err(|error| {
                                format!("invalid face on line {}: {error}", line_index + 1)
                            })?;
                        index
                            .checked_sub(1)
                            .filter(|index| *index < positions.len())
                            .ok_or_else(|| {
                                format!("face index out of range on line {}", line_index + 1)
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if polygon.len() < 3 {
                    return Err(format!(
                        "face on line {} has fewer than 3 vertices",
                        line_index + 1
                    ));
                }
                polygons.push(polygon);
            }
            _ => {}
        }
    }

    if positions.is_empty() || polygons.is_empty() {
        return Err("OBJ has no usable geometry".to_owned());
    }

    normalize_positions(&mut positions)?;
    // `normalize_positions` centers the bounding box at the scene origin.
    let center = [0.0; 3];
    let mut normals = vec![[0.0; 3]; positions.len()];
    let mut triangles = Vec::<[usize; 3]>::new();

    for polygon in &polygons {
        for offset in 1..polygon.len() - 1 {
            let triangle = [polygon[0], polygon[offset], polygon[offset + 1]];
            let Some(normal) = triangle_normal(
                positions[triangle[0]],
                positions[triangle[1]],
                positions[triangle[2]],
            ) else {
                continue;
            };
            for index in triangle {
                normals[index] = add(normals[index], normal);
            }
            triangles.push(triangle);
        }
    }

    for (index, normal) in normals.iter_mut().enumerate() {
        *normal = normalize(*normal)
            .or_else(|| normalize(subtract(positions[index], center)))
            .ok_or_else(|| "head mesh contains an invalid vertex normal".to_owned())?;
    }

    let vertex_count = triangles
        .len()
        .checked_mul(3)
        .ok_or_else(|| "head mesh is too large".to_owned())?;
    let vertex_count = u32::try_from(vertex_count).map_err(|_| "head mesh is too large")?;
    let mut output = Vec::with_capacity(12 + vertex_count as usize * 6 * size_of::<f32>());
    output.extend_from_slice(MAGIC);
    output.extend_from_slice(&vertex_count.to_le_bytes());
    for triangle in triangles {
        for index in triangle {
            for value in positions[index].into_iter().chain(normals[index]) {
                output.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok(output)
}

fn normalize_positions(positions: &mut [[f32; 3]]) -> Result<(), String> {
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    for position in positions.iter() {
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(position[axis]);
            maximum[axis] = maximum[axis].max(position[axis]);
        }
    }
    let height = maximum[1] - minimum[1];
    if !height.is_finite() || height <= f32::EPSILON {
        return Err("head mesh has invalid bounds".to_owned());
    }
    let center = scale(add(minimum, maximum), 0.5);
    let mesh_scale = TARGET_HEIGHT / height;
    for position in positions {
        *position = scale(subtract(*position, center), mesh_scale);
    }
    Ok(())
}

fn triangle_normal(first: [f32; 3], second: [f32; 3], third: [f32; 3]) -> Option<[f32; 3]> {
    normalize(cross(subtract(second, first), subtract(third, first)))
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length_squared = dot(vector, vector);
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return None;
    }
    Some(scale(vector, length_squared.sqrt().recip()))
}

fn add(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn subtract(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn scale(vector: [f32; 3], factor: f32) -> [f32; 3] {
    [vector[0] * factor, vector[1] * factor, vector[2] * factor]
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0].mul_add(right[0], left[1].mul_add(right[1], left[2] * right[2]))
}

fn cross(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1].mul_add(right[2], -left[2] * right[1]),
        left[2].mul_add(right[0], -left[0] * right[2]),
        left[0].mul_add(right[1], -left[1] * right[0]),
    ]
}
