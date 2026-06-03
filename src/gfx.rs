//! Graphics 그룹 — GPU 인프라 (wgpu) + terminal cell renderer.
//!
//! - [`gpu`]: wgpu GpuState (device/queue/surface), canvas texture cache, egui bridge, render pass orchestration.
//! - [`renderer`]: terminal cell 그리드의 GPU 렌더링 (CellRenderer + palette + pipeline).

pub mod gpu;
pub mod perf;
pub mod renderer;
