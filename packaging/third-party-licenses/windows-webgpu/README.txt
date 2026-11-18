Third-party components in the VoxCtrl Windows installer
========================================================

The Windows build of VoxCtrl accelerates the Moonshine and Parakeet speech
engines through ONNX Runtime's WebGPU execution provider. That provider needs
the libraries below at run time, so the installer places them next to
voxctrl.exe. They are shipped unmodified, exactly as they come with the
prebuilt ONNX Runtime that VoxCtrl is built against.

webgpu_dawn.dll
    Dawn, an implementation of the WebGPU standard.
    Copyright The Dawn & Tint Authors. BSD 3-Clause License.
    https://dawn.googlesource.com/dawn
    Licence text: Dawn-LICENSE.txt

dxcompiler.dll
    DirectX Shader Compiler, based on LLVM/Clang.
    Copyright Microsoft Corporation and the LLVM contributors.
    University of Illinois/NCSA Open Source License (LLVM Release License).
    https://github.com/microsoft/DirectXShaderCompiler
    Licence text: DirectXShaderCompiler-LICENSE.txt
    Third-party notices: DirectXShaderCompiler-ThirdPartyNotices.txt

Every file in this installer is under an open-source licence. In particular it
does not include dxil.dll, Microsoft's closed-source DXIL signing library: the
Dawn that VoxCtrl ships signs shaders with dxcompiler.dll alone, and the
release build fails if a Dawn that still loads dxil.dll ever comes back.

ONNX Runtime itself is linked into voxctrl.exe. Its MIT licence and
third-party notices are the onnxruntime-*.txt files in this folder.

VoxCtrl itself is MIT licensed; see the project's LICENSE file. The full list
of open-source projects VoxCtrl builds on is in Settings -> About.
