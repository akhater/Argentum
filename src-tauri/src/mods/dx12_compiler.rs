//! Which compiler turns the shaders into DirectX 12 code. Ours.
//!
//! wgpu compiles every pipeline when a GPU processor is built. On DirectX 12
//! it uses FXC, the compiler in every Windows, unless it finds DXC, which
//! Windows does not ship. Measured 2026-11-05, Intel Arc 140V, one headless
//! export: building the processor took 15 s with FXC and 1.7 s with DXC, so
//! a DirectX 12 user waited 15 s at every launch before the first photo.
//!
//! The app carries DXC in its resources when it is packaged with it
//! (`dxcompiler.dll` and `dxil.dll`, microsoft/DirectXShaderCompiler, MIT and
//! LLVM licences). Without them, wgpu's own choice stands: DXC on the PATH,
//! else FXC. `WGPU_DX12_COMPILER` still overrides both.

use std::path::Path;

/// DXC's library, in the app's resources folder.
pub const DXC_DLL: &str = "dxcompiler.dll";

/// Use the DXC in `resources` for DirectX 12, when it is there.
pub fn choose(instance: &mut wgpu::InstanceDescriptor, resources: &Path) {
    if std::env::var_os("WGPU_DX12_COMPILER").is_some() {
        return;
    }
    let dxc = resources.join(DXC_DLL);
    if dxc.is_file() {
        log::info!("DirectX 12 shaders compile with {}", dxc.display());
        instance.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::DynamicDxc {
            dxc_path: dxc.to_string_lossy().into_owned(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_dxc_in_the_resources_leaves_wgpus_choice() {
        let mut instance = wgpu::InstanceDescriptor::new_without_display_handle();
        let before = format!("{:?}", instance.backend_options.dx12.shader_compiler);
        choose(&mut instance, Path::new("a folder that is not there"));
        assert_eq!(
            format!("{:?}", instance.backend_options.dx12.shader_compiler),
            before
        );
    }
}
