use std::{borrow::Cow, error::Error, fmt};

use glam::{Mat4, Vec3};
use web_sys::HtmlCanvasElement;
use wgpu::util::DeviceExt;

use crate::{
    camera::{CameraError, CameraPreset, OrbitCamera, SOURCE_SPHERE_RADIUS},
    head,
};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;
const VERTEX_STRIDE: wgpu::BufferAddress = 10 * size_of::<f32>() as wgpu::BufferAddress;

pub struct Renderer {
    canvas: HtmlCanvasElement,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth_view: wgpu::TextureView,
    triangle_pipeline: wgpu::RenderPipeline,
    glow_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    camera: OrbitCamera,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    head_buffer: wgpu::Buffer,
    head_vertex_count: u32,
    measurement_buffer: wgpu::Buffer,
    measurement_vertex_count: u32,
    grid_buffer: wgpu::Buffer,
    grid_vertex_count: u32,
    axes_buffer: wgpu::Buffer,
    axes_vertex_count: u32,
    contributor_buffer: wgpu::Buffer,
    contributor_vertex_count: u32,
    contributor_line_buffer: wgpu::Buffer,
    contributor_line_vertex_count: u32,
    marker_buffer: wgpu::Buffer,
    marker_vertex_count: u32,
    marker_glow_buffer: wgpu::Buffer,
    marker_glow_vertex_count: u32,
    visible_layers: u8,
    backend_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayLayer {
    Measurements,
    Grid,
    Axes,
    ContributorGuides,
}

impl DisplayLayer {
    const fn mask(self) -> u8 {
        match self {
            Self::Measurements => 1 << 0,
            Self::Grid => 1 << 1,
            Self::Axes => 1 << 2,
            Self::ContributorGuides => 1 << 3,
        }
    }
}

const ALL_DISPLAY_LAYERS: u8 = (1 << 4) - 1;

impl Renderer {
    #[allow(clippy::too_many_lines)]
    pub async fn new(
        canvas: HtmlCanvasElement,
        selected_direction: Vec3,
        measurement_directions: &[Vec3],
        contributors: &[(Vec3, f32)],
    ) -> Result<Self, RendererError> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL;
        let instance = wgpu::util::new_instance_with_webgpu_detection(instance_descriptor).await;
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
            .map_err(|error| {
                RendererError::new(format!("failed to create canvas surface: {error}"))
            })?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .map_err(|error| {
                RendererError::new(format!("failed to request WebGPU adapter: {error}"))
            })?;
        let backend_name = format!("{:?}", adapter.get_info().backend);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("binaural-explorer-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                ..Default::default()
            })
            .await
            .map_err(|error| {
                RendererError::new(format!("failed to request WebGPU device: {error}"))
            })?;
        let config = surface
            .get_default_config(&adapter, 1, 1)
            .ok_or_else(|| RendererError::new("canvas surface has no supported configuration"))?;
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shader.wgsl"))),
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene-pipeline-layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let triangle_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::TriangleList,
            true,
            wgpu::BlendState::ALPHA_BLENDING,
        );
        let glow_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::TriangleList,
            false,
            additive_blend_state(),
        );
        let line_pipeline = create_pipeline(
            &device,
            &pipeline_layout,
            &shader,
            config.format,
            wgpu::PrimitiveTopology::LineList,
            false,
            wgpu::BlendState::ALPHA_BLENDING,
        );

        let camera = OrbitCamera::new();
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("camera-buffer"),
            contents: &matrix_bytes(camera.view_projection()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind-group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let head_vertices = head::vertices()
            .map_err(RendererError::new)?
            .into_iter()
            .map(|vertex| Vertex {
                position: vertex.position,
                normal: vertex.normal,
                color: [0.72, 0.73, 0.76, 1.0],
            })
            .collect::<Vec<_>>();
        let head_vertex_count = vertex_count(&head_vertices)?;
        let head_buffer = vertex_buffer(&device, "head-buffer", &head_vertices, false);
        let measurement_vertices = measurement_vertices(measurement_directions);
        let measurement_vertex_count = vertex_count(&measurement_vertices)?;
        let measurement_buffer =
            vertex_buffer(&device, "measurement-buffer", &measurement_vertices, false);
        let grid_vertices = sphere_grid_vertices();
        let grid_vertex_count = vertex_count(&grid_vertices)?;
        let grid_buffer = vertex_buffer(&device, "grid-buffer", &grid_vertices, false);
        let axes_vertices = coordinate_axis_vertices();
        let axes_vertex_count = vertex_count(&axes_vertices)?;
        let axes_buffer = vertex_buffer(&device, "axes-buffer", &axes_vertices, false);
        let initial_contributor_vertices = contributor_vertices(contributors);
        let contributor_vertex_count = vertex_count(&initial_contributor_vertices)?;
        let contributor_capacity =
            contributor_vertices(&[(Vec3::X, 1.0), (Vec3::Y, 1.0), (Vec3::Z, 1.0)]).len();
        let contributor_buffer =
            dynamic_vertex_buffer(&device, "contributor-buffer", contributor_capacity)?;
        queue.write_buffer(
            &contributor_buffer,
            0,
            &vertex_bytes(&initial_contributor_vertices),
        );
        let contributor_line_vertices = contributor_line_vertices(selected_direction, contributors);
        let contributor_line_vertex_count = vertex_count(&contributor_line_vertices)?;
        let contributor_line_buffer = dynamic_vertex_buffer(&device, "contributor-line-buffer", 6)?;
        queue.write_buffer(
            &contributor_line_buffer,
            0,
            &vertex_bytes(&contributor_line_vertices),
        );
        let marker_vertices = marker_core_vertices(selected_direction);
        let marker_vertex_count = vertex_count(&marker_vertices)?;
        let marker_buffer = vertex_buffer(&device, "marker-buffer", &marker_vertices, true);
        let marker_glow_vertices = marker_glow_vertices(selected_direction);
        let marker_glow_vertex_count = vertex_count(&marker_glow_vertices)?;
        let marker_glow_buffer =
            vertex_buffer(&device, "marker-glow-buffer", &marker_glow_vertices, true);
        let depth_view = create_depth_view(&device, config.width, config.height);

        let renderer = Self {
            canvas,
            surface,
            device,
            queue,
            config,
            depth_view,
            triangle_pipeline,
            glow_pipeline,
            line_pipeline,
            camera,
            camera_buffer,
            camera_bind_group,
            head_buffer,
            head_vertex_count,
            measurement_buffer,
            measurement_vertex_count,
            grid_buffer,
            grid_vertex_count,
            axes_buffer,
            axes_vertex_count,
            contributor_buffer,
            contributor_vertex_count,
            contributor_line_buffer,
            contributor_line_vertex_count,
            marker_buffer,
            marker_vertex_count,
            marker_glow_buffer,
            marker_glow_vertex_count,
            visible_layers: ALL_DISPLAY_LAYERS,
            backend_name,
        };
        renderer.render()?;
        Ok(renderer)
    }

    pub fn resize(
        &mut self,
        css_width: f32,
        css_height: f32,
        device_pixel_ratio: f32,
    ) -> Result<(), RendererError> {
        if !css_width.is_finite()
            || !css_height.is_finite()
            || !device_pixel_ratio.is_finite()
            || css_width <= 0.0
            || css_height <= 0.0
            || device_pixel_ratio <= 0.0
        {
            return Err(RendererError::new("invalid canvas dimensions"));
        }
        let requested_width = f64::from(css_width) * f64::from(device_pixel_ratio);
        let requested_height = f64::from(css_height) * f64::from(device_pixel_ratio);
        let max_dimension = f64::from(self.device.limits().max_texture_dimension_2d);
        let resolution_scale = (max_dimension / requested_width.max(requested_height)).min(1.0);
        let physical_width = (requested_width * resolution_scale).round().max(1.0);
        let physical_height = (requested_height * resolution_scale).round().max(1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let (width, height) = (physical_width as u32, physical_height as u32);
        if width == self.config.width && height == self.config.height {
            return Ok(());
        }
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth_view = create_depth_view(&self.device, width, height);
        self.camera.set_aspect(css_width / css_height);
        self.update_camera();
        self.render()
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) -> Result<(), RendererError> {
        if !delta_x.is_finite() || !delta_y.is_finite() {
            return Err(RendererError::new("orbit delta must be finite"));
        }
        self.camera.orbit(delta_x, delta_y);
        self.update_camera();
        self.render()
    }

    pub fn zoom(&mut self, wheel_delta: f32) -> Result<(), RendererError> {
        if !wheel_delta.is_finite() {
            return Err(RendererError::new("zoom delta must be finite"));
        }
        self.camera.zoom(wheel_delta);
        self.update_camera();
        self.render()
    }

    pub fn set_camera_preset(&mut self, preset: CameraPreset) -> Result<(), RendererError> {
        self.camera.set_preset(preset);
        self.update_camera();
        self.render()
    }

    pub fn set_display_layer(
        &mut self,
        layer: DisplayLayer,
        visible: bool,
    ) -> Result<(), RendererError> {
        if visible {
            self.visible_layers |= layer.mask();
        } else {
            self.visible_layers &= !layer.mask();
        }
        self.render()
    }

    pub fn navigate_camera(
        &mut self,
        delta_x: f32,
        delta_y: f32,
        wheel_delta: f32,
    ) -> Result<(), RendererError> {
        if !delta_x.is_finite() || !delta_y.is_finite() || !wheel_delta.is_finite() {
            return Err(RendererError::new("camera navigation delta must be finite"));
        }
        self.camera.orbit(delta_x, delta_y);
        self.camera.zoom(wheel_delta);
        self.update_camera();
        self.render()
    }

    pub fn pick_direction(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Result<Vec3, RendererError> {
        self.camera
            .ray_from_canvas(x, y, width, height)?
            .sphere_intersection_direction(Vec3::ZERO, SOURCE_SPHERE_RADIUS)
            .map_err(Into::into)
    }

    pub fn set_selection(
        &mut self,
        direction: Vec3,
        contributors: &[(Vec3, f32)],
    ) -> Result<(), RendererError> {
        if contributors.len() > 3 {
            return Err(RendererError::new(
                "renderer supports at most three interpolation contributors",
            ));
        }
        let bytes = vertex_bytes(&marker_core_vertices(direction));
        self.queue.write_buffer(&self.marker_buffer, 0, &bytes);
        let glow_bytes = vertex_bytes(&marker_glow_vertices(direction));
        self.queue
            .write_buffer(&self.marker_glow_buffer, 0, &glow_bytes);
        let contributor_vertices = contributor_vertices(contributors);
        self.contributor_vertex_count = vertex_count(&contributor_vertices)?;
        self.queue.write_buffer(
            &self.contributor_buffer,
            0,
            &vertex_bytes(&contributor_vertices),
        );
        let line_vertices = contributor_line_vertices(direction, contributors);
        self.contributor_line_vertex_count = vertex_count(&line_vertices)?;
        self.queue.write_buffer(
            &self.contributor_line_buffer,
            0,
            &vertex_bytes(&line_vertices),
        );
        Ok(())
    }

    pub fn backend_name(&self) -> &str {
        &self.backend_name
    }

    pub fn render(&self) -> Result<(), RendererError> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err(RendererError::new("WebGPU canvas surface was lost"));
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(RendererError::new(
                    "WebGPU canvas surface validation failed",
                ));
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene-command-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.027,
                            g: 0.043,
                            b: 0.067,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_pipeline(&self.triangle_pipeline);
            pass.set_vertex_buffer(0, self.head_buffer.slice(..));
            pass.draw(0..self.head_vertex_count, 0..1);
            if self.layer_is_visible(DisplayLayer::Measurements) {
                pass.set_vertex_buffer(0, self.measurement_buffer.slice(..));
                pass.draw(0..self.measurement_vertex_count, 0..1);
            }
            pass.set_vertex_buffer(0, self.contributor_buffer.slice(..));
            pass.draw(0..self.contributor_vertex_count, 0..1);
            pass.set_vertex_buffer(0, self.marker_buffer.slice(..));
            pass.draw(0..self.marker_vertex_count, 0..1);
            pass.set_pipeline(&self.line_pipeline);
            if self.layer_is_visible(DisplayLayer::Grid) {
                pass.set_vertex_buffer(0, self.grid_buffer.slice(..));
                pass.draw(0..self.grid_vertex_count, 0..1);
            }
            if self.layer_is_visible(DisplayLayer::Axes) {
                pass.set_vertex_buffer(0, self.axes_buffer.slice(..));
                pass.draw(0..self.axes_vertex_count, 0..1);
            }
            if self.layer_is_visible(DisplayLayer::ContributorGuides) {
                pass.set_vertex_buffer(0, self.contributor_line_buffer.slice(..));
                pass.draw(0..self.contributor_line_vertex_count, 0..1);
            }
            pass.set_pipeline(&self.glow_pipeline);
            pass.set_vertex_buffer(0, self.marker_glow_buffer.slice(..));
            pass.draw(0..self.marker_glow_vertex_count, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }

    fn update_camera(&self) {
        self.queue.write_buffer(
            &self.camera_buffer,
            0,
            &matrix_bytes(self.camera.view_projection()),
        );
    }

    fn layer_is_visible(&self, layer: DisplayLayer) -> bool {
        self.visible_layers & layer.mask() != 0
    }
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    color_format: wgpu::TextureFormat,
    topology: wgpu::PrimitiveTopology,
    depth_write_enabled: bool,
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("scene-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 3 * size_of::<f32>() as wgpu::BufferAddress,
                        shader_location: 1,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 6 * size_of::<f32>() as wgpu::BufferAddress,
                        shader_location: 2,
                    },
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(depth_write_enabled),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn additive_blend_state() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent::OVER,
    }
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("scene-depth-texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

#[derive(Clone, Copy)]
struct Vertex {
    position: Vec3,
    normal: Vec3,
    color: [f32; 4],
}

fn vertex_buffer(
    device: &wgpu::Device,
    label: &'static str,
    vertices: &[Vertex],
    copy_destination: bool,
) -> wgpu::Buffer {
    let mut usage = wgpu::BufferUsages::VERTEX;
    if copy_destination {
        usage |= wgpu::BufferUsages::COPY_DST;
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &vertex_bytes(vertices),
        usage,
    })
}

fn dynamic_vertex_buffer(
    device: &wgpu::Device,
    label: &'static str,
    vertex_capacity: usize,
) -> Result<wgpu::Buffer, RendererError> {
    let capacity = u64::try_from(vertex_capacity)
        .ok()
        .and_then(|count| count.checked_mul(VERTEX_STRIDE))
        .ok_or_else(|| RendererError::new("dynamic scene buffer is too large"))?;
    Ok(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }))
}

fn vertex_count(vertices: &[Vertex]) -> Result<u32, RendererError> {
    u32::try_from(vertices.len()).map_err(|_| RendererError::new("scene has too many vertices"))
}

fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 10 * size_of::<f32>());
    for vertex in vertices {
        for value in vertex
            .position
            .to_array()
            .into_iter()
            .chain(vertex.normal.to_array())
            .chain(vertex.color)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

fn matrix_bytes(matrix: Mat4) -> Vec<u8> {
    matrix
        .to_cols_array()
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect()
}

fn sphere_grid_vertices() -> Vec<Vertex> {
    let mut vertices = Vec::new();
    let grid = [0.27, 0.58, 0.72, 0.35];
    let segments = 64_u16;
    for elevation in [-60.0_f32, -30.0, 0.0, 30.0, 60.0] {
        let phi = elevation.to_radians();
        for segment in 0_u16..segments {
            let a0 = std::f32::consts::TAU * f32::from(segment) / f32::from(segments);
            let a1 = std::f32::consts::TAU * f32::from(segment + 1) / f32::from(segments);
            push_line(
                &mut vertices,
                sphere_point(a0, phi),
                sphere_point(a1, phi),
                grid,
            );
        }
    }
    for longitude in (0_u16..360).step_by(30) {
        let theta = f32::from(longitude).to_radians();
        for segment in 0_u16..segments / 2 {
            let p0 = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::PI * f32::from(segment) / f32::from(segments / 2);
            let p1 = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::PI * f32::from(segment + 1) / f32::from(segments / 2);
            push_line(
                &mut vertices,
                sphere_point(theta, p0),
                sphere_point(theta, p1),
                grid,
            );
        }
    }
    vertices
}

fn coordinate_axis_vertices() -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(30);
    push_axis_arrow(
        &mut vertices,
        Vec3::X,
        Vec3::Y,
        Vec3::Z,
        [0.95, 0.25, 0.28, 1.0],
    );
    push_axis_arrow(
        &mut vertices,
        Vec3::Y,
        Vec3::X,
        Vec3::Z,
        [0.30, 0.90, 0.45, 1.0],
    );
    push_axis_arrow(
        &mut vertices,
        Vec3::Z,
        Vec3::X,
        Vec3::Y,
        [0.30, 0.65, 1.0, 1.0],
    );
    vertices
}

fn push_axis_arrow(
    vertices: &mut Vec<Vertex>,
    direction: Vec3,
    first_wing_axis: Vec3,
    second_wing_axis: Vec3,
    color: [f32; 4],
) {
    let tip = direction * 1.9;
    let arrow_base = direction * 1.73;
    push_line(vertices, Vec3::ZERO, tip, color);
    for wing in [
        first_wing_axis * 0.07,
        first_wing_axis * -0.07,
        second_wing_axis * 0.07,
        second_wing_axis * -0.07,
    ] {
        push_line(vertices, tip, arrow_base + wing, color);
    }
}

fn sphere_point(theta: f32, phi: f32) -> Vec3 {
    SOURCE_SPHERE_RADIUS * Vec3::new(phi.cos() * theta.sin(), phi.sin(), phi.cos() * theta.cos())
}

fn measurement_vertices(directions: &[Vec3]) -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(directions.len() * 24);
    for &direction in directions {
        append_octahedron(
            &mut vertices,
            direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS,
            0.010,
            [0.48, 0.58, 0.66, 0.82],
        );
    }
    vertices
}

fn contributor_vertices(contributors: &[(Vec3, f32)]) -> Vec<Vertex> {
    let mut vertices = Vec::new();
    for &(direction, weight) in contributors {
        append_sphere(
            &mut vertices,
            direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS,
            0.032 + 0.020 * weight.clamp(0.0, 1.0),
            [1.0, 0.46, 0.08, 1.0],
            true,
        );
    }
    vertices
}

fn contributor_line_vertices(direction: Vec3, contributors: &[(Vec3, f32)]) -> Vec<Vertex> {
    let source = direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS;
    let mut vertices = Vec::with_capacity(contributors.len() * 2);
    for &(contributor_direction, weight) in contributors {
        let color = [1.0, 0.55, 0.08, 0.55 + 0.45 * weight.clamp(0.0, 1.0)];
        push_line(
            &mut vertices,
            source,
            contributor_direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS,
            color,
        );
    }
    vertices
}

fn append_octahedron(vertices: &mut Vec<Vertex>, center: Vec3, radius: f32, color: [f32; 4]) {
    let points = [
        center + Vec3::X * radius,
        center - Vec3::X * radius,
        center + Vec3::Y * radius,
        center - Vec3::Y * radius,
        center + Vec3::Z * radius,
        center - Vec3::Z * radius,
    ];
    for [first, second, third] in [
        [2, 0, 4],
        [2, 4, 1],
        [2, 1, 5],
        [2, 5, 0],
        [3, 4, 0],
        [3, 1, 4],
        [3, 5, 1],
        [3, 0, 5],
    ] {
        vertices.extend([first, second, third].map(|index| Vertex {
            position: points[index],
            normal: Vec3::ZERO,
            color,
        }));
    }
}

fn marker_core_vertices(direction: Vec3) -> Vec<Vertex> {
    let mut vertices = Vec::new();
    append_sphere(
        &mut vertices,
        direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS,
        0.072,
        [0.04, 0.48, 1.0, 1.0],
        true,
    );
    vertices
}

fn marker_glow_vertices(direction: Vec3) -> Vec<Vertex> {
    let mut vertices = Vec::new();
    let center = direction.normalize_or_zero() * SOURCE_SPHERE_RADIUS;
    for (radius, alpha) in [
        (0.088, 0.085),
        (0.100, 0.065),
        (0.114, 0.048),
        (0.130, 0.034),
        (0.148, 0.023),
        (0.170, 0.014),
        (0.194, 0.007),
    ] {
        append_sphere(
            &mut vertices,
            center,
            radius,
            [0.0, 0.32, 1.0, alpha],
            false,
        );
    }
    vertices
}

fn append_sphere(
    vertices: &mut Vec<Vertex>,
    center: Vec3,
    radius: f32,
    color: [f32; 4],
    lit: bool,
) {
    const LATITUDE_STEPS: u16 = 12;
    const LONGITUDE_STEPS: u16 = 20;
    for latitude in 0..LATITUDE_STEPS {
        let phi0 = -std::f32::consts::FRAC_PI_2
            + std::f32::consts::PI * f32::from(latitude) / f32::from(LATITUDE_STEPS);
        let phi1 = -std::f32::consts::FRAC_PI_2
            + std::f32::consts::PI * f32::from(latitude + 1) / f32::from(LATITUDE_STEPS);
        for longitude in 0..LONGITUDE_STEPS {
            let theta0 = std::f32::consts::TAU * f32::from(longitude) / f32::from(LONGITUDE_STEPS);
            let theta1 =
                std::f32::consts::TAU * f32::from(longitude + 1) / f32::from(LONGITUDE_STEPS);
            let directions = [
                unit_sphere_point(theta0, phi0),
                unit_sphere_point(theta1, phi0),
                unit_sphere_point(theta1, phi1),
                unit_sphere_point(theta0, phi1),
            ];
            push_sphere_triangle(
                vertices,
                center,
                radius,
                color,
                lit,
                [directions[0], directions[1], directions[2]],
            );
            push_sphere_triangle(
                vertices,
                center,
                radius,
                color,
                lit,
                [directions[0], directions[2], directions[3]],
            );
        }
    }
}

fn unit_sphere_point(theta: f32, phi: f32) -> Vec3 {
    Vec3::new(phi.cos() * theta.sin(), phi.sin(), phi.cos() * theta.cos())
}

fn push_sphere_triangle(
    vertices: &mut Vec<Vertex>,
    center: Vec3,
    radius: f32,
    color: [f32; 4],
    lit: bool,
    directions: [Vec3; 3],
) {
    vertices.extend(directions.map(|direction| Vertex {
        position: center + direction * radius,
        normal: if lit { direction } else { Vec3::ZERO },
        color,
    }));
}

fn push_line(vertices: &mut Vec<Vertex>, first: Vec3, second: Vec3, color: [f32; 4]) {
    vertices.extend([
        Vertex {
            position: first,
            normal: Vec3::ZERO,
            color,
        },
        Vertex {
            position: second,
            normal: Vec3::ZERO,
            color,
        },
    ]);
}

#[derive(Debug)]
pub struct RendererError(String);

impl RendererError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for RendererError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for RendererError {}

impl From<CameraError> for RendererError {
    fn from(error: CameraError) -> Self {
        Self::new(error.to_string())
    }
}
