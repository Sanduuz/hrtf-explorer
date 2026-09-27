use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 8] = b"HMESH001";
const TARGET_HEIGHT: f32 = 1.25;
const SUBDIVISION_LEVELS: usize = 2;
const LOWER_TRIM_FRACTION: f32 = 0.43;
const EAR_LATERAL_THRESHOLD: f32 = 0.86;
const HEAD_VERTICAL_OFFSET: f32 = 0.12;

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

    for _ in 0..SUBDIVISION_LEVELS {
        (positions, polygons) = catmull_clark(&positions, &polygons)?;
    }
    (positions, polygons) = trim_lower_mesh(&positions, &polygons)?;
    normalize_positions(&mut positions)?;
    // `normalize_positions` places the ear axis at the acoustic scene origin.
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
    let ear_y = ear_axis_y(positions)?;
    let center = [
        f32::midpoint(minimum[0], maximum[0]),
        ear_y,
        f32::midpoint(minimum[2], maximum[2]),
    ];
    let mesh_scale = TARGET_HEIGHT / height;
    for position in positions {
        *position = scale(subtract(*position, center), mesh_scale);
        position[1] += HEAD_VERTICAL_OFFSET;
    }
    Ok(())
}

fn ear_axis_y(positions: &[[f32; 3]]) -> Result<f32, String> {
    let minimum_x = positions
        .iter()
        .map(|position| position[0])
        .reduce(f32::min)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let maximum_x = positions
        .iter()
        .map(|position| position[0])
        .reduce(f32::max)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let minimum_y = positions
        .iter()
        .map(|position| position[1])
        .reduce(f32::min)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let maximum_y = positions
        .iter()
        .map(|position| position[1])
        .reduce(f32::max)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let lateral_center = f32::midpoint(minimum_x, maximum_x);
    let upper_head_start = f32::midpoint(minimum_y, maximum_y);
    let ear_radius = positions
        .iter()
        .filter(|position| position[1] >= upper_head_start)
        .map(|position| (position[0] - lateral_center).abs())
        .reduce(f32::max)
        .ok_or_else(|| "head mesh has no upper-head positions".to_owned())?;
    if !ear_radius.is_finite() || ear_radius <= f32::EPSILON {
        return Err("head mesh has invalid lateral bounds".to_owned());
    }

    // The bust's shoulders are wider than its ears. Restricting the landmark
    // search to the upper half isolates the pinnae; averaging the lateral
    // silhouette on both sides gives a stable, symmetric ear-height axis.
    let outer_ear_threshold = ear_radius * EAR_LATERAL_THRESHOLD;
    let outer_ear_points = positions.iter().filter_map(|position| {
        (position[1] >= upper_head_start
            && (position[0] - lateral_center).abs() >= outer_ear_threshold)
            .then_some(*position)
    });
    average_vectors(outer_ear_points).map(|average| average[1])
}

type EdgeKey = (usize, usize);
type SubdividedMesh = (Vec<[f32; 3]>, Vec<Vec<usize>>);

fn trim_lower_mesh(
    positions: &[[f32; 3]],
    polygons: &[Vec<usize>],
) -> Result<SubdividedMesh, String> {
    let minimum_y = positions
        .iter()
        .map(|position| position[1])
        .reduce(f32::min)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let maximum_y = positions
        .iter()
        .map(|position| position[1])
        .reduce(f32::max)
        .ok_or_else(|| "head mesh has no positions".to_owned())?;
    let trim_y = (maximum_y - minimum_y).mul_add(LOWER_TRIM_FRACTION, minimum_y);
    let mut clipped_positions = positions.to_vec();
    let mut intersections = HashMap::<EdgeKey, usize>::new();
    let mut clipped_polygons = Vec::with_capacity(polygons.len());

    for polygon in polygons {
        let mut clipped = Vec::with_capacity(polygon.len() + 2);
        for (corner, &current) in polygon.iter().enumerate() {
            let next = polygon[(corner + 1) % polygon.len()];
            let current_inside = positions[current][1] >= trim_y;
            let next_inside = positions[next][1] >= trim_y;
            if current_inside {
                clipped.push(current);
            }
            if current_inside != next_inside {
                let edge = edge_key(current, next);
                let intersection = *intersections.entry(edge).or_insert_with(|| {
                    let first = positions[current];
                    let second = positions[next];
                    let amount = (trim_y - first[1]) / (second[1] - first[1]);
                    let point = add(first, scale(subtract(second, first), amount));
                    let index = clipped_positions.len();
                    clipped_positions.push(point);
                    index
                });
                clipped.push(intersection);
            }
        }
        if clipped.len() >= 3 {
            clipped_polygons.push(clipped);
        }
    }

    compact_mesh(&clipped_positions, clipped_polygons)
}

fn compact_mesh(
    positions: &[[f32; 3]],
    mut polygons: Vec<Vec<usize>>,
) -> Result<SubdividedMesh, String> {
    let mut remapping = vec![usize::MAX; positions.len()];
    let mut compact_positions = Vec::new();
    for polygon in &mut polygons {
        for index in polygon {
            let mapped = remapping
                .get_mut(*index)
                .ok_or_else(|| "head mesh contains an invalid clipped index".to_owned())?;
            if *mapped == usize::MAX {
                *mapped = compact_positions.len();
                compact_positions.push(positions[*index]);
            }
            *index = *mapped;
        }
    }
    if compact_positions.is_empty() || polygons.is_empty() {
        return Err("head mesh trim removed all geometry".to_owned());
    }
    Ok((compact_positions, polygons))
}

fn edge_key(first: usize, second: usize) -> EdgeKey {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

#[allow(clippy::too_many_lines)]
fn catmull_clark(
    positions: &[[f32; 3]],
    polygons: &[Vec<usize>],
) -> Result<SubdividedMesh, String> {
    let face_points = polygons
        .iter()
        .map(|polygon| average_indices(positions, polygon))
        .collect::<Result<Vec<_>, _>>()?;
    let mut edge_faces = HashMap::<EdgeKey, Vec<usize>>::new();
    let mut vertex_faces = vec![Vec::<usize>::new(); positions.len()];
    let mut vertex_edges = vec![Vec::<EdgeKey>::new(); positions.len()];

    for (face_index, polygon) in polygons.iter().enumerate() {
        for (corner, &vertex) in polygon.iter().enumerate() {
            let next = polygon[(corner + 1) % polygon.len()];
            let edge = edge_key(vertex, next);
            edge_faces.entry(edge).or_default().push(face_index);
            vertex_faces[vertex].push(face_index);
            vertex_edges[vertex].push(edge);
            vertex_edges[next].push(edge);
        }
    }
    for edges in &mut vertex_edges {
        edges.sort_unstable();
        edges.dedup();
    }
    if edge_faces.values().any(|faces| faces.len() > 2) {
        return Err("head mesh contains a non-manifold edge".to_owned());
    }

    let mut subdivided_positions = Vec::with_capacity(
        positions
            .len()
            .saturating_add(edge_faces.len())
            .saturating_add(polygons.len()),
    );
    for (index, &position) in positions.iter().enumerate() {
        let boundary_neighbors = vertex_edges[index]
            .iter()
            .filter(|edge| edge_faces[*edge].len() == 1)
            .map(|&(first, second)| if first == index { second } else { first })
            .collect::<Vec<_>>();
        let smoothed = if boundary_neighbors.is_empty() {
            let faces = &vertex_faces[index];
            let edges = &vertex_edges[index];
            if faces.len() < 3 || faces.len() != edges.len() {
                return Err("head mesh contains an invalid interior vertex".to_owned());
            }
            let face_average = average_vectors(faces.iter().map(|&face| face_points[face]))?;
            let edge_average = average_vectors(
                edges
                    .iter()
                    .map(|&(first, second)| scale(add(positions[first], positions[second]), 0.5)),
            )?;
            let count = u16::try_from(faces.len())
                .map(f32::from)
                .map_err(|_| "head mesh vertex has too many connected faces")?;
            scale(
                add(
                    add(face_average, scale(edge_average, 2.0)),
                    scale(position, count - 3.0),
                ),
                count.recip(),
            )
        } else {
            if boundary_neighbors.len() != 2 {
                return Err("head mesh contains an invalid boundary vertex".to_owned());
            }
            add(
                scale(position, 0.75),
                scale(
                    add(
                        positions[boundary_neighbors[0]],
                        positions[boundary_neighbors[1]],
                    ),
                    0.125,
                ),
            )
        };
        subdivided_positions.push(smoothed);
    }

    let mut edges = edge_faces.keys().copied().collect::<Vec<_>>();
    edges.sort_unstable();
    let mut edge_indices = HashMap::<EdgeKey, usize>::with_capacity(edges.len());
    for edge in edges {
        let faces = &edge_faces[&edge];
        let point = if faces.len() == 2 {
            scale(
                add(
                    add(positions[edge.0], positions[edge.1]),
                    add(face_points[faces[0]], face_points[faces[1]]),
                ),
                0.25,
            )
        } else {
            scale(add(positions[edge.0], positions[edge.1]), 0.5)
        };
        let index = subdivided_positions.len();
        subdivided_positions.push(point);
        edge_indices.insert(edge, index);
    }

    let face_point_offset = subdivided_positions.len();
    subdivided_positions.extend(face_points);
    let mut subdivided_polygons = Vec::new();
    for (face_index, polygon) in polygons.iter().enumerate() {
        for (corner, &vertex) in polygon.iter().enumerate() {
            let previous = polygon[(corner + polygon.len() - 1) % polygon.len()];
            let next = polygon[(corner + 1) % polygon.len()];
            subdivided_polygons.push(vec![
                vertex,
                edge_indices[&edge_key(vertex, next)],
                face_point_offset + face_index,
                edge_indices[&edge_key(previous, vertex)],
            ]);
        }
    }
    Ok((subdivided_positions, subdivided_polygons))
}

fn average_indices(positions: &[[f32; 3]], indices: &[usize]) -> Result<[f32; 3], String> {
    average_vectors(indices.iter().map(|&index| positions[index]))
}

fn average_vectors(vectors: impl IntoIterator<Item = [f32; 3]>) -> Result<[f32; 3], String> {
    let mut sum = [0.0; 3];
    let mut count = 0_u16;
    for vector in vectors {
        sum = add(sum, vector);
        count = count
            .checked_add(1)
            .ok_or_else(|| "head mesh has too many connected elements".to_owned())?;
    }
    if count == 0 {
        return Err("head mesh contains an empty element".to_owned());
    }
    Ok(scale(sum, f32::from(count).recip()))
}

fn triangle_normal(first: [f32; 3], second: [f32; 3], third: [f32; 3]) -> Option<[f32; 3]> {
    let normal = cross(subtract(second, first), subtract(third, first));
    let length_squared = dot(normal, normal);
    if !length_squared.is_finite() || length_squared <= f32::MIN_POSITIVE {
        return None;
    }
    // Accumulating the unnormalized face normals weights each contribution by
    // triangle area and produces smoother shading around uneven topology.
    Some(normal)
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length_squared = dot(vector, vector);
    if !length_squared.is_finite() || length_squared <= f32::MIN_POSITIVE {
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
