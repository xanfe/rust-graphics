use wgpu::{BindGroup, Instance};
use winit::{
    event::*,
    window::  Window,
};
use cgmath::prelude::*;
use crate::{render::{
    pipelines::figure::{FigureVertex, FigurePipeline, FigureLayout, Instance as FigureInstance},
    texture::Texture,
    mesh::{Mesh, Quad, Cube},
    model::Model,
    buffer::Buffer
    
}, scene::camera::{Camera, CameraUniform, Projection, CameraLayout, CameraController}};

use super::model_obj::{DrawModel, self};

use crate::common::resources;

/// State gestiona los recursos de renderizado de la aplicación,
/// actualmente para un triángulo. Con la expansión del proyecto,
/// se podría renombrar a Renderer y crear un GlobalState para un
/// alcance más amplio.
pub struct State {
    camera: Camera,
    projection: Projection,
    camera_uniform: CameraUniform,
    camera_buffer: Buffer<CameraUniform>,
    instance_buffer: Buffer<FigureInstance>,
    pub camera_controller: CameraController,
    camera_bind_group: wgpu::BindGroup,
    surface: wgpu::Surface,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,
    pub window: Window,
    quad_pipeline:FigurePipeline,
    quad_model: Model<FigureVertex>,
    instances: Vec<FigureInstance>,
    diffuse_bind_group: wgpu::BindGroup,
    depth_texture: Texture,
    diffuse_texture: Texture, //for later usage
    pub mouse_pressed: bool,
    obj_model: model_obj::Model
    
}
 
impl State {
    pub async fn new(window: Window) -> Self {
        let camera_controller = CameraController::new(4.0, 2.0);
        let size = window.inner_size();


        const NUM_INSTANCES_PER_ROW: u32 = 10;
        const INSTANCE_DISPLACEMENT: cgmath::Vector3<f32> = cgmath::Vector3::new(NUM_INSTANCES_PER_ROW as f32 * 0.5, 0.0, NUM_INSTANCES_PER_ROW as f32 * 0.5);
        
        let instances = (0..NUM_INSTANCES_PER_ROW).flat_map(|z| {
            (0..NUM_INSTANCES_PER_ROW).map(move |x| {
                let position = cgmath::Vector3 { x: x as f32, y: 0.0, z: z as f32 } - INSTANCE_DISPLACEMENT;

                let rotation = if position.is_zero() {
                    // this is needed so an object at (0, 0, 0) won't get scaled to zero
                    // as Quaternions can affect scale if they're not created correctly
                    cgmath::Quaternion::from_axis_angle(cgmath::Vector3::unit_z(), cgmath::Deg(0.0))
                } else {
                    cgmath::Quaternion::from_axis_angle(position.normalize(), cgmath::Deg(0.0))
                };

                FigureInstance::new(position, rotation)
            })
        }).collect::<Vec<_>>();

        

        // The instance is a handle to our GPU
        // BackendBit::PRIMARY => Vulkan + Metal + DX12 + Browser WebGPU
        let wgpu_instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // # Safety
        // The surface needs to live as long as the window that created it.
        // State owns the window so this should be safe.
        let surface = unsafe { wgpu_instance.create_surface(&window) }.unwrap();

        let adapter = wgpu_instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: None,
                    features: wgpu::Features::empty(),
                    limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let instance_buffer = Buffer::new(&device, wgpu::BufferUsages::VERTEX, &instances);
        
        let surface_caps = surface.get_capabilities(&adapter);
        // Shader code in this tutorial assumes an Srgb surface texture. Using a different
        // one will result all the colors comming out darker. If you want to support non
        // Srgb surfaces, you'll need to account for that when drawing to the frame.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,//investigar cual es la diferencia entre esto y usar surface_caps.usages
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let camera = Camera::new((0.0, 5.0, 10.0), cgmath::Deg(-90.0), cgmath::Deg(-20.0));
        let projection = Projection::new(config.width, config.height, cgmath::Deg(45.0), 0.1, 100.0);

        let camera_layout = CameraLayout::new(&device);

        let mut camera_uniform = CameraUniform::new();
        camera_uniform.update_view_proj(&camera, &projection);

        let camera_buffer = Buffer::new(&device, wgpu::BufferUsages::UNIFORM, &[camera_uniform]);

        

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &camera_layout.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.buff.as_entire_binding(),
                }
            ],
            label: Some("camera_bind_group"),
        });


        let diffuse_bytes = include_bytes!("../../assets/images/dirt.png");
        let diffuse_texture = Texture::from_bytes(&device, &queue, diffuse_bytes, "dirt.png").unwrap();
        let depth_texture = Texture::create_depth_texture(&device, &config, "depth_texture");
        let figure_layout = FigureLayout::new(&device);

        let diffuse_bind_group = device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                layout: &figure_layout.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                    }
                ],
                label: Some("diffuse_bind_group"),
            }
        );

        let shader = device.create_shader_module(wgpu::include_wgsl!("../../assets/shaders/shader.wgsl"));

        let mut quad_mesh = Mesh::new();

        // quad_mesh.push_quad(Quad::new(
        //     Vertex { position: [-0.0868241, 0.49240386, 0.0], tex_coords: [0.4131759, 0.00759614], },
        //     Vertex { position: [-0.49513406, 0.06958647, 0.0], tex_coords: [0.0048659444, 0.43041354], },
        //     Vertex { position: [-0.21918549, -0.44939706, 0.0], tex_coords: [0.28081453, 0.949397], },
        //     Vertex { position: [0.35966998, -0.3473291, 0.0], tex_coords: [0.85967, 0.84732914], }
        // ));

        // quad_mesh.push(Vertex { position: [0.85966998, -0.2473291, 0.0], tex_coords: [0.85967, 0.84732914], });



        // Definir los vértices del cubo directamente.
        // Vértices de la base inferior
        // Definiciones de vértices con coordenadas UV únicas para cada cara del cubo.
        // Corrección de las coordenadas UV para las caras superior e inferior del cubo.
        // Coordenadas UV ajustadas para las caras superior e inferior.
        // Coordenadas UV ajustadas para las caras superior e inferior.

        let a = FigureVertex { position: [0.0, 0.0, 0.0], tex_coords: [0.0, 1.0] }; // bottom-left of bottom face
        let b = FigureVertex { position: [1.0, 0.0, 0.0], tex_coords: [1.0, 1.0] }; // bottom-right of bottom face
        let c = FigureVertex { position: [1.0, 1.0, 0.0], tex_coords: [1.0, 0.0] }; // top-right of bottom face
        let d = FigureVertex { position: [0.0, 1.0, 0.0], tex_coords: [0.0, 0.0] }; // top-left of bottom face

        let e = FigureVertex { position: [0.0, 0.0, 1.0], tex_coords: [0.0, 1.0] }; // bottom-left of top face
        let f = FigureVertex { position: [1.0, 0.0, 1.0], tex_coords: [1.0, 1.0] }; // bottom-right of top face
        let g = FigureVertex { position: [1.0, 1.0, 1.0], tex_coords: [1.0, 0.0] }; // top-right of top face
        let h = FigureVertex { position: [0.0, 1.0, 1.0], tex_coords: [0.0, 0.0] }; // top-left of top face

        // Crear un cubo con los vértices definidos.
        let cube = Cube::new(a, b, c, d, e, f, g, h);


        let obj_model =
        resources::load_model("cube.obj", &device, &queue, &figure_layout.bind_group_layout)
            .await
            .unwrap();
 





        quad_mesh.push_cube(cube);

        let quad_model = Model::new(&device, &quad_mesh).unwrap();

        let quad_pipeline: FigurePipeline = FigurePipeline::new(
            &device,
            &shader,
            &config,
            &figure_layout,
            &camera_layout //temporary until i add global layouts
        );

        Self {
            camera,
            camera_uniform,
            camera_buffer,
            projection,
            instances,
            instance_buffer,
            camera_controller,
            camera_bind_group,
            surface,
            device,
            queue,
            config,
            size,
            window,
            quad_pipeline,
            quad_model,
            diffuse_bind_group,
            diffuse_texture,
            depth_texture,
            mouse_pressed: false, // NEW!
            obj_model
        }
    }

    pub fn window(&self) -> &Window {
        &self.window
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.projection.resize(new_size.width, new_size.height);
        self.depth_texture = Texture::create_depth_texture(&self.device, &self.config, "depth_texture");
        if new_size.width > 0 && new_size.height > 0 {
            self.size = new_size;
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn input(&mut self, event: &WindowEvent) -> bool {

        match event {
            WindowEvent::KeyboardInput {
                input:
                KeyboardInput {
                    state,
                    virtual_keycode: Some(key),
                    ..
                },
                ..
            } => self.camera_controller.process_keyboard(*key, *state),
            WindowEvent::MouseWheel { delta, .. } => {
                self.camera_controller.process_scroll(delta);
                true
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state,
                ..
            } => {
                self.mouse_pressed = *state == ElementState::Pressed;
                true
            }
            _ => false
        }
    }

    pub fn update(&mut self,  dt: instant::Duration) {
        self.camera_controller.update_camera(&mut self.camera, dt);
        self.camera_uniform.update_view_proj(&self.camera, &self.projection);
        self.queue.write_buffer(&self.camera_buffer.buff, 0, bytemuck::cast_slice(&[self.camera_uniform]));
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });
        {
            
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { 
                            r: 0.5,
                            g: 0.5,
                            b: 1.0,
                            a: 1.0
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_texture.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            render_pass.set_pipeline(&self.quad_pipeline.pipeline);
            render_pass.set_bind_group(0, &self.diffuse_bind_group, &[]);
            render_pass.set_bind_group(1, &self.camera_bind_group, &[]);
            //render_pass.set_vertex_buffer(0, self.quad_model.vbuf().slice(..));
            //render_pass.set_vertex_buffer(1, self.instance_buffer.buff.slice(..));
            // render_pass.set_index_buffer(self.quad_model.ibuf().slice(..), wgpu::IndexFormat::Uint16);
            // render_pass.draw_indexed(0..self.quad_model.num_indices, 0, 0..1 as _);

            render_pass.draw_mesh_instanced(&self.obj_model.meshes[0], 0..self.instances.len() as u32);
        }

        // submit will accept anything that implements IntoIter
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }
}
