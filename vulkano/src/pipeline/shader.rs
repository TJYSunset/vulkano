// Copyright (c) 2016 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

//! Stage of a graphics pipeline.
//!
//! In Vulkan, shaders are grouped in *shader modules*. Each shader module is built from SPIR-V
//! code and can contain one or more entry points. Note that for the moment the official
//! GLSL-to-SPIR-V compiler does not support multiple entry points.
//!
//! The vulkano library does not provide any functionnality that checks and introspects the SPIR-V
//! code, therefore the whole shader-related API is unsafe. You are encouraged to use the
//! `vulkano-shaders` crate that will generate Rust code that wraps around vulkano's shaders API.

use std::borrow::Cow;
use std::error;
use std::ffi::CStr;
use std::fmt;
use std::iter;
use std::iter::Empty as EmptyIter;
use std::marker::PhantomData;
use std::mem;
use std::ops::Range;
use std::ptr;
use std::sync::Arc;

use descriptor::pipeline_layout::EmptyPipelineDesc;
use descriptor::pipeline_layout::PipelineLayoutDescNames;
use format::Format;
use pipeline::input_assembly::PrimitiveTopology;

use OomError;
use VulkanObject;
use check_errors;
use device::Device;
use vk;

/// Contains SPIR-V code with one or more entry points.
///
/// Note that it is advised to wrap around a `ShaderModule` with a struct that is different for
/// each shader.
#[derive(Debug)]
pub struct ShaderModule {
    // The module.
    module: vk::ShaderModule,
    // Pointer to the device.
    device: Arc<Device>,
}

impl ShaderModule {
    /// Builds a new shader module from SPIR-V.
    ///
    /// # Safety
    ///
    /// - The SPIR-V code is not validated.
    /// - The SPIR-V code may require some features that are not enabled. This isn't checked by
    ///   this function either.
    ///
    pub unsafe fn new(device: Arc<Device>, spirv: &[u8]) -> Result<Arc<ShaderModule>, OomError> {
        debug_assert!((spirv.len() % 4) == 0);

        let module = {
            let infos = vk::ShaderModuleCreateInfo {
                sType: vk::STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
                pNext: ptr::null(),
                flags: 0, // reserved
                codeSize: spirv.len(),
                pCode: spirv.as_ptr() as *const _,
            };

            let vk = device.pointers();
            let mut output = mem::uninitialized();
            check_errors(vk.CreateShaderModule(device.internal_object(),
                                               &infos,
                                               ptr::null(),
                                               &mut output))?;
            output
        };

        Ok(Arc::new(ShaderModule {
                        module: module,
                        device: device,
                    }))
    }

    /// Gets access to an entry point contained in this module.
    ///
    /// This is purely a *logical* operation. It returns a struct that *represents* the entry
    /// point but doesn't actually do anything.
    ///
    /// # Safety
    ///
    /// - The user must check that the entry point exists in the module, as this is not checked
    ///   by Vulkan.
    /// - The input, output and layout must correctly describe the input, output and layout used
    ///   by this stage.
    ///
    pub unsafe fn graphics_entry_point<'a, S, I, O, L>(&'a self, name: &'a CStr, input: I, output: O,
                                                       layout: L, ty: GraphicsShaderType)
                                                       -> GraphicsEntryPoint<'a, S, I, O, L>
    {
        GraphicsEntryPoint {
            module: self,
            name: name,
            input: input,
            output: output,
            layout: layout,
            ty: ty,
            marker: PhantomData,
        }
    }

    /// Gets access to an entry point contained in this module.
    ///
    /// This is purely a *logical* operation. It returns a struct that *represents* the entry
    /// point but doesn't actually do anything.
    ///
    /// # Safety
    ///
    /// - The user must check that the entry point exists in the module, as this is not checked
    ///   by Vulkan.
    /// - The layout must correctly describe the layout used by this stage.
    ///
    #[inline]
    pub unsafe fn compute_entry_point<'a, S, L>(&'a self, name: &'a CStr, layout: L)
                                                -> ComputeEntryPoint<'a, S, L> {
        ComputeEntryPoint {
            module: self,
            name: name,
            layout: layout,
            marker: PhantomData,
        }
    }
}

unsafe impl VulkanObject for ShaderModule {
    type Object = vk::ShaderModule;

    #[inline]
    fn internal_object(&self) -> vk::ShaderModule {
        self.module
    }
}

impl Drop for ShaderModule {
    #[inline]
    fn drop(&mut self) {
        unsafe {
            let vk = self.device.pointers();
            vk.DestroyShaderModule(self.device.internal_object(), self.module, ptr::null());
        }
    }
}

pub unsafe trait GraphicsEntryPointAbstract: EntryPointAbstract {
    type InputDefinition: ShaderInterfaceDef;
    type OutputDefinition: ShaderInterfaceDef;

    /// Returns the input attributes used by the shader stage.
    fn input(&self) -> &Self::InputDefinition;

    /// Returns the output attributes used by the shader stage.
    fn output(&self) -> &Self::OutputDefinition;

    /// Returns the type of shader.
    fn ty(&self) -> GraphicsShaderType;
}

/// Represents a shader entry point in a shader module.
///
/// Can be obtained by calling `entry_point()` on the shader module.
#[derive(Debug, Copy, Clone)]
pub struct GraphicsEntryPoint<'a, S, I, O, L> {
    module: &'a ShaderModule,
    name: &'a CStr,
    input: I,
    layout: L,
    output: O,
    ty: GraphicsShaderType,
    marker: PhantomData<S>,
}

unsafe impl<'a, S, I, O, L> EntryPointAbstract for GraphicsEntryPoint<'a, S, I, O, L>
    where L: PipelineLayoutDescNames,
          I: ShaderInterfaceDef,
          O: ShaderInterfaceDef,
          S: SpecializationConstants,
{
    type PipelineLayout = L;
    type SpecializationConstants = S;

    #[inline]
    fn module(&self) -> &ShaderModule {
        self.module
    }

    #[inline]
    fn name(&self) -> &CStr {
        self.name
    }

    #[inline]
    fn layout(&self) -> &L {
        &self.layout
    }
}

unsafe impl<'a, S, I, O, L> GraphicsEntryPointAbstract for GraphicsEntryPoint<'a, S, I, O, L>
    where L: PipelineLayoutDescNames,
          I: ShaderInterfaceDef,
          O: ShaderInterfaceDef,
          S: SpecializationConstants,
{
    type InputDefinition = I;
    type OutputDefinition = O;

    #[inline]
    fn input(&self) -> &I {
        &self.input
    }

    #[inline]
    fn output(&self) -> &O {
        &self.output
    }

    #[inline]
    fn ty(&self) -> GraphicsShaderType {
        self.ty
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GraphicsShaderType {
    Vertex,
    TessellationControl,
    TessellationEvaluation,
    Geometry(GeometryShaderExecutionMode),
    Fragment,
}

/// Declares which type of primitives are expected by the geometry shader.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum GeometryShaderExecutionMode {
    Points,
    Lines,
    LinesWithAdjacency,
    Triangles,
    TrianglesWithAdjacency,
}

impl GeometryShaderExecutionMode {
    /// Returns true if the given primitive topology can be used with this execution mode.
    #[inline]
    pub fn matches(&self, input: PrimitiveTopology) -> bool {
        match (*self, input) {
            (GeometryShaderExecutionMode::Points, PrimitiveTopology::PointList) => true,
            (GeometryShaderExecutionMode::Lines, PrimitiveTopology::LineList) => true,
            (GeometryShaderExecutionMode::Lines, PrimitiveTopology::LineStrip) => true,
            (GeometryShaderExecutionMode::LinesWithAdjacency,
             PrimitiveTopology::LineListWithAdjacency) => true,
            (GeometryShaderExecutionMode::LinesWithAdjacency,
             PrimitiveTopology::LineStripWithAdjacency) => true,
            (GeometryShaderExecutionMode::Triangles, PrimitiveTopology::TriangleList) => true,
            (GeometryShaderExecutionMode::Triangles, PrimitiveTopology::TriangleStrip) => true,
            (GeometryShaderExecutionMode::Triangles, PrimitiveTopology::TriangleFan) => true,
            (GeometryShaderExecutionMode::TrianglesWithAdjacency,
             PrimitiveTopology::TriangleListWithAdjacency) => true,
            (GeometryShaderExecutionMode::TrianglesWithAdjacency,
             PrimitiveTopology::TriangleStripWithAdjacency) => true,
            _ => false,
        }
    }
}

pub unsafe trait EntryPointAbstract {
    type PipelineLayout: PipelineLayoutDescNames;
    type SpecializationConstants: SpecializationConstants;

    /// Returns the module this entry point comes from.
    fn module(&self) -> &ShaderModule;

    /// Returns the name of the entry point.
    fn name(&self) -> &CStr;

    /// Returns the pipeline layout used by the shader stage.
    fn layout(&self) -> &Self::PipelineLayout;
}

/// Represents the entry point of a compute shader in a shader module.
///
/// Can be obtained by calling `compute_shader_entry_point()` on the shader module.
#[derive(Debug, Copy, Clone)]
pub struct ComputeEntryPoint<'a, S, L> {
    module: &'a ShaderModule,
    name: &'a CStr,
    layout: L,
    marker: PhantomData<S>,
}

unsafe impl<'a, S, L> EntryPointAbstract for ComputeEntryPoint<'a, S, L>
    where L: PipelineLayoutDescNames,
          S: SpecializationConstants,
{
    type PipelineLayout = L;
    type SpecializationConstants = S;

    #[inline]
    fn module(&self) -> &ShaderModule {
        self.module
    }

    #[inline]
    fn name(&self) -> &CStr {
        self.name
    }

    #[inline]
    fn layout(&self) -> &L {
        &self.layout
    }
}

/// A dummy that implements `GraphicsEntryPointAbstract` and `EntryPointAbstract`.
///
/// When a function has a signature like: `fn foo<S: EntryPointAbstract>(shader: Option<S>)`, you
/// can pass `None::<EmptyEntryPointDummy>`.
///
/// This object is meant to be a replacement to `!` before it is stabilized.
// TODO: ^
#[derive(Debug, Copy, Clone)]
pub enum EmptyEntryPointDummy {
}

unsafe impl EntryPointAbstract for EmptyEntryPointDummy {
    type PipelineLayout = EmptyPipelineDesc;
    type SpecializationConstants = ();

    #[inline]
    fn module(&self) -> &ShaderModule {
        unreachable!()
    }

    #[inline]
    fn name(&self) -> &CStr {
        unreachable!()
    }

    #[inline]
    fn layout(&self) -> &EmptyPipelineDesc {
        unreachable!()
    }
}

unsafe impl GraphicsEntryPointAbstract for EmptyEntryPointDummy {
    type InputDefinition = EmptyShaderInterfaceDef;
    type OutputDefinition = EmptyShaderInterfaceDef;

    #[inline]
    fn input(&self) -> &EmptyShaderInterfaceDef {
        unreachable!()
    }

    #[inline]
    fn output(&self) -> &EmptyShaderInterfaceDef {
        unreachable!()
    }

    #[inline]
    fn ty(&self) -> GraphicsShaderType {
        unreachable!()
    }
}

/// Types that contain the definition of an interface between two shader stages, or between
/// the outside and a shader stage.
///
/// # Safety
///
/// - Must only provide one entry per location.
/// - The format of each element must not be larger than 128 bits.
///
pub unsafe trait ShaderInterfaceDef {
    /// Iterator returned by `elements`.
    type Iter: ExactSizeIterator<Item = ShaderInterfaceDefEntry>;

    /// Iterates over the elements of the interface.
    fn elements(&self) -> Self::Iter;
}

/// Entry of a shader interface definition.
#[derive(Debug, Clone)]
pub struct ShaderInterfaceDefEntry {
    /// Range of locations covered by the element.
    pub location: Range<u32>,
    /// Format of a each location of the element.
    pub format: Format,
    /// Name of the element, or `None` if the name is unknown.
    pub name: Option<Cow<'static, str>>,
}

/// Description of an empty shader interface.
#[derive(Debug, Copy, Clone)]
pub struct EmptyShaderInterfaceDef;

unsafe impl ShaderInterfaceDef for EmptyShaderInterfaceDef {
    type Iter = EmptyIter<ShaderInterfaceDefEntry>;

    #[inline]
    fn elements(&self) -> Self::Iter {
        iter::empty()
    }
}

/// Extension trait for `ShaderInterfaceDef` that specifies that the interface is potentially
/// compatible with another one.
pub unsafe trait ShaderInterfaceDefMatch<I>: ShaderInterfaceDef
    where I: ShaderInterfaceDef
{
    /// Returns `Ok` if the two definitions match.
    fn matches(&self, other: &I) -> Result<(), ShaderInterfaceMismatchError>;
}

// TODO: turn this into a default impl that can be specialized
unsafe impl<T, I> ShaderInterfaceDefMatch<I> for T
    where T: ShaderInterfaceDef,
          I: ShaderInterfaceDef
{
    fn matches(&self, other: &I) -> Result<(), ShaderInterfaceMismatchError> {
        if self.elements().len() != other.elements().len() {
            return Err(ShaderInterfaceMismatchError::ElementsCountMismatch);
        }

        for a in self.elements() {
            for loc in a.location.clone() {
                let b = match other
                    .elements()
                    .find(|e| loc >= e.location.start && loc < e.location.end) {
                    None => return Err(ShaderInterfaceMismatchError::MissingElement {
                                           location: loc,
                                       }),
                    Some(b) => b,
                };

                if a.format != b.format {
                    return Err(ShaderInterfaceMismatchError::FormatMismatch);
                }

                // TODO: enforce this?
                /*match (a.name, b.name) {
                    (Some(ref an), Some(ref bn)) => if an != bn { return false },
                    _ => ()
                };*/
            }
        }

        Ok(())
    }
}

/// Error that can happen when the interface mismatches between two shader stages.
// TODO: improve diagnostic a bit
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShaderInterfaceMismatchError {
    ElementsCountMismatch,
    MissingElement { location: u32 },
    FormatMismatch,
}

impl error::Error for ShaderInterfaceMismatchError {
    #[inline]
    fn description(&self) -> &str {
        match *self {
            ShaderInterfaceMismatchError::ElementsCountMismatch =>
                "the number of elements mismatches",
            ShaderInterfaceMismatchError::MissingElement { .. } => "an element is missing",
            ShaderInterfaceMismatchError::FormatMismatch =>
                "the format of an element does not match",
        }
    }
}

impl fmt::Display for ShaderInterfaceMismatchError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}", error::Error::description(self))
    }
}

/// Trait for types that contain specialization data for shaders.
///
/// It is implemented on `()` for shaders that don't have any specialization constant.
///
/// # Safety
///
/// - The `SpecializationMapEntry` returned must contain valid offsets and sizes.
///
pub unsafe trait SpecializationConstants {
    /// Returns descriptors of the struct's layout.
    fn descriptors() -> &'static [SpecializationMapEntry];
}

unsafe impl SpecializationConstants for () {
    #[inline]
    fn descriptors() -> &'static [SpecializationMapEntry] {
        &[]
    }
}

/// Describes an indiviual constant to set in the shader. Also a field in the struct.
// Has the same memory representation as a `VkSpecializationMapEntry`.
#[repr(C)]
pub struct SpecializationMapEntry {
    /// Identifier of the constant in the shader that corresponds to this field.
    pub constant_id: u32,
    /// Offset within this struct for the data.
    pub offset: u32,
    /// Size of the data in bytes.
    pub size: usize,
}
