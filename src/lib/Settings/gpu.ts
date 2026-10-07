/** How each GPU backend id from `accelerator_support` is named in the UI. */
export const GPU_LABELS: Record<string, string> = {
  cuda: "CUDA (NVIDIA)",
  vulkan: "Vulkan (AMD/Intel/NVIDIA)",
  coreml: "CoreML (Apple)",
  webgpu: "WebGPU (AMD/Intel/NVIDIA)",
};

export const gpuLabel = (id: string): string => GPU_LABELS[id] ?? id;
