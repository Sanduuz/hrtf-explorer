struct Camera {
    view_projection: mat4x4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.color = input.color;
    output.normal = input.normal;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal_length = length(input.normal);
    if normal_length < 0.5 {
        return input.color;
    }

    let normal = input.normal / normal_length;
    let key_light = normalize(vec3<f32>(-0.45, 0.7, 0.65));
    let fill_light = normalize(vec3<f32>(0.7, 0.15, 0.3));
    let diffuse = max(dot(normal, key_light), 0.0);
    let fill = max(dot(normal, fill_light), 0.0);
    let light = 0.22 + 0.68 * diffuse + 0.18 * fill;
    return vec4<f32>(input.color.rgb * light, input.color.a);
}
