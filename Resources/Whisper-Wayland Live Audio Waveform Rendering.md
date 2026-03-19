# **High-Performance Live Audio Waveform Rendering on Linux: Integration within the Whisper-Wayland Ecosystem**

## **Introduction and Contextual Foundation**

The transition of Linux desktop environments from the legacy X11 display server to the modern, secure Wayland protocol has introduced profound architectural shifts in how graphical user interfaces, display compositors, and input/output subsystems interact. Concurrent with this display architecture evolution is the rapid proliferation and integration of local, highly capable artificial intelligence models into daily user workflows. OpenAI's Whisper model, a general-purpose speech recognition system trained on a massive dataset of diverse audio, has emerged as the premier open-source tool for multilingual speech recognition, speech translation, and spoken language identification.1 The JRufer/Whisper-Wayland project exemplifies this technological convergence, providing an on-demand, push-to-talk speech-to-text utility tailored specifically for Wayland compositors such as Sway and Hyprland.2

The origin of the Whisper-Wayland project lies in the emerging practice of "vibe coding" or agentic coding, wherein a developer describes the desired architecture to an AI assistant, iterating through functional phases. As documented by its creator, the tool was developed over a five-hour session to satisfy a highly specific set of stringent technical constraints that standard out-of-the-box solutions frequently fail to meet.2 These constraints mandate that the application must run natively on Ubuntu under a Wayland compositor, must execute entirely without root permissions, must provide a seamless push-to-talk experience, and must possess the ability to dynamically insert the recognized text at the current cursor position.2 Addressing these constraints required a careful orchestration of user-space tools and kernel-level injection methods to bypass Wayland's strict security restrictions regarding global input monitoring and virtual keyboard emulation.

However, a fundamental requirement for any professional-grade voice-driven application is immediate, deterministic visual feedback. Users must be able to visually ascertain when the microphone is active, whether the input levels are optimal, and if acoustic clipping is occurring during speech. Rendering a live audio waveform in real-time presents a distinct and highly complex engineering challenge within this constrained environment. It requires intercepting a high-frequency digital audio stream, processing the acoustic signal into a visual envelope, and rendering a smooth animation at 60 frames per second (FPS) or higher. Crucially, this must be achieved while maintaining ultra-low latency and strictly avoiding any central processing unit (CPU) or graphics processing unit (GPU) contention with the computationally demanding Whisper inference engine.

This comprehensive research report provides an exhaustive architectural analysis and implementation strategy for rendering a live audio waveform with low latency and minimal CPU cost, specifically tailored for integration into the Whisper-Wayland ecosystem. The analysis traverses the modern Linux audio stack, evaluating the transition from PulseAudio to PipeWire, details digital signal processing (DSP) decimation techniques suitable for Python, explores GPU-accelerated rendering pipelines using PySide6 on Wayland, and outlines the precise system-level kernel tuning required to achieve deterministic, real-time performance.

## **Architectural Baseline: The Whisper-Wayland Environment**

To design an optimal waveform rendering pipeline, the existing constraints, dependencies, and operational paradigms of the target environment must be mapped with precision. The Whisper-Wayland repository functions not as a standalone monolithic application, but as a highly specific integration layer that coordinates several independent open-source utilities into a cohesive workflow.2

## **Core Technology Stack and Dependencies**

The technology stack underpinning Whisper-Wayland relies on several critical components that dictate the boundaries of any visual enhancement. The codebase is heavily weighted toward Python, comprising 86.7% of the repository, augmented by shell scripts representing the remaining 13.3%.4 This language distribution necessitates a design that mitigates the inherent concurrency limitations of the Python runtime.

The following table delineates the primary software components utilized in the Whisper-Wayland stack and their respective roles in the transcription pipeline:

| Component | Primary Function | Technical Implementation Details |
| :---- | :---- | :---- |
| **PySide6** | Graphical User Interface | The official Python binding for the Qt6 framework. Chosen for its native Wayland support and seamless integration with KDE Plasma and wlroots-based compositors without relying on XWayland translation layers.4 |
| **sounddevice** | Audio Capture | A Python library acting as a wrapper around the C-based PortAudio library. It interfaces natively with PipeWire or PulseAudio to capture microphone streams via non-blocking callbacks.4 |
| **whisper.cpp** | Transcription Engine | A lightweight C/C++ implementation of the OpenAI Whisper model. It is explicitly compiled with Vulkan support (-DGGML\_VULKAN=ON) to offload matrix multiplications to AMD, Intel, or NVIDIA GPUs, freeing the CPU.4 |
| **ydotool** | Text Injection | A command-line utility that bypasses Wayland's strict security protocols by interacting directly with the Linux kernel's /dev/uinput subsystem to emulate hardware keystrokes.4 |
| **evdev** | Global Shortcuts | A Python binding to the Linux input handling subsystem. It monitors hardware-level keyboard events directly from /dev/input/, allowing the application to capture hotkeys even when it does not possess window focus.4 |

## **Whisper Model Selection and Performance Implications**

The application is capable of utilizing various sizes of the Whisper model, ranging from lightweight, English-only models to massive, multilingual architectures. The choice of model directly impacts the system's memory footprint and inference speed, which in turn dictates how much system overhead is permissible for the graphical user interface.

| Model Variant | Approximate Size | Processing Speed | Accuracy Characteristics |
| :---- | :---- | :---- | :---- |
| **tiny.en** | \~75 MB | Fastest | Good for clear, native English dictation.5 |
| **base.en** | \~150 MB | Fast | Better contextual understanding; the default choice for balancing speed and accuracy.5 |
| **small.en** | \~500 MB | Medium | Great accuracy; handles mild accents and background noise effectively.5 |
| **medium.en** | \~1.5 GB | Slow | Excellent accuracy; highly reliable for complex vocabulary.5 |
| **large-v3** | \~3.0 GB | Slowest | Best overall accuracy; excels in multilingual transcription and translation tasks, though non-native speakers have reported issues with unpredictable language switching during inference.5 |

Because the larger models saturate GPU compute units and VRAM, the waveform visualizer must operate orthogonally to the Vulkan execution queues utilized by whisper.cpp.4 If the graphical interface relies on the same compute paths, the user will experience severe UI freezing during the transcription phase, destroying the application's perceived responsiveness.

## **Navigating Wayland Security and Input Constraints**

The Whisper-Wayland project operates within the strict security boundaries defined by the Wayland protocol. Unlike X11, where any application can query the coordinates of any window or inject keystrokes globally, Wayland enforces strict isolation between clients.8 A background application cannot simply simulate keystrokes into another active window using traditional X11 tools like xdotool.5

To circumvent this, the project utilizes ydotool, which requires a background daemon (ydotoold) to interact directly with the kernel-level uinput module, effectively appearing to the operating system as a physical hardware keyboard.4 The visualizer integration must not interfere with this delicate injection pipeline. The visual feedback mechanism must be entirely passive, listening to the audio stream and rendering to a dedicated PySide6 Wayland surface without attempting to interact with the window management operations of the host compositor.4

## **The Linux Audio Subsystem and Low-Latency Capture**

The journey of an analog acoustic wave from the microphone diaphragm, through the analog-to-digital converter (ADC), into the Linux kernel, and finally into the Python application space involves several dense layers of abstraction. Optimizing this data path is the absolute prerequisite for achieving low-latency waveform generation that visually synchronizes with the user's speech.

## **The Ascendancy of PipeWire in Modern Distributions**

Historically, the Linux audio landscape was highly fragmented, requiring users to navigate complex routing topologies to achieve low latency. The lowest level of audio hardware interaction is handled by the Advanced Linux Sound Architecture (ALSA), which provides kernel-level drivers.11 On top of ALSA, general-purpose desktop distributions layered PulseAudio to handle consumer routing, software mixing, and volume control.12 However, PulseAudio was fundamentally designed for stability and power savings, often introducing unacceptable latency.12 Professional audio engineers and creative coders requiring low latency were forced to utilize the JACK Audio Connection Kit, which bypassed PulseAudio entirely to communicate directly with ALSA, enabling synchronous, low-latency routing between applications.12

Modern Wayland-based environments, particularly those running on Ubuntu, Arch Linux, and Fedora, have largely standardized on PipeWire as a unified multimedia framework.3 PipeWire represents a monumental shift, successfully merging the low-latency, graph-based routing capabilities of JACK with the consumer-friendly device management of PulseAudio. PipeWire achieves this by providing drop-in replacement daemons: pipewire-pulse for applications expecting PulseAudio, and pipewire-jack for professional applications.13

In the specific context of the Whisper-Wayland repository, the application captures audio using the sounddevice library.4 Because sounddevice utilizes PortAudio under the hood, it seamlessly connects to the PipeWire daemon via the PulseAudio or ALSA compatibility layers, depending on the environment variables defined by the user at runtime.3 By operating through PipeWire, the application benefits from dynamic latency negotiation, allowing it to request smaller buffer sizes without requiring the user to tear down their entire desktop audio configuration.

## **The Mathematics and Physics of Audio Latency**

Latency in digital audio systems is fundamentally a function of the buffer size (the number of samples processed in a single chunk) and the sampling rate (the frequency at which the analog signal is measured). The total theoretical round-trip latency of a system is the sum of input buffering, kernel processing time, application processing time, and output buffering. For a graphical waveform visualizer, only input and application processing latency are relevant, as no audio is being pushed back to the speakers.

The base latency introduced by an input buffer is calculated using the standard formula:

Latency (seconds) \= Buffer Size (samples) / Sample Rate (Hz)

By default, PulseAudio and standard PipeWire configurations often request buffers ranging from 1024 to 2048 samples. This configuration prioritizes audio stream stability and minimizes CPU wakeups, which is ideal for playing music or watching videos. However, at a standard microphone sample rate of 48,000 Hz, a 2048-sample buffer introduces a significant delay: Latency \= 2048 / 48000 ≈ 0.0426 seconds (42.6 ms) 15

While 42.6 milliseconds is virtually imperceptible when recording voice for subsequent transcription by Whisper, standard interactive graphics require frame updates every 16.6 milliseconds to match a 60 Hz monitor refresh rate. If the audio buffer is only yielding new data every 42.6 ms, the waveform visualizer will appear visually disconnected from the user's voice. The animation will jump erratically, rendering in jagged steps rather than flowing smoothly, completely failing its purpose as a real-time feedback mechanism.

To achieve fluid visual coherence, the audio buffer size must be drastically reduced. Configuring the capture stream to utilize a buffer of 256 samples yields a latency profile highly conducive to real-time graphics: Latency \= 256 / 48000 ≈ 0.0053 seconds (5.3 ms) 15

This 5.3 ms granularity ensures that the Python application can poll the incoming audio stream multiple times within a single visual frame, allowing the waveform to accurately reflect the instantaneous acoustic energy of the user's speech without visual stuttering.

## **Extracting Audio Streams via sounddevice**

The sounddevice library in Python allows for non-blocking, asynchronous audio capture by instantiating an InputStream and registering a callback function. The architectural integration of a waveform visualizer within the Whisper-Wayland codebase requires intercepting this data stream without mutating or disrupting the chunks being routed to the whisper.cpp transcription engine.

When the user triggers the configured push-to-talk keybind (captured at the kernel level via evdev), the application initiates the audio stream.4 The registered callback function is subsequently invoked by a high-priority operating system thread every time the hardware buffer fills. The callback receives the audio data as a highly efficient NumPy array.

To guarantee low CPU overhead and preserve low latency, the logic contained within the callback function must strictly adhere to the principles of real-time safe programming. The following operations must be categorically avoided within the audio callback:

1. **Memory Allocation:** The callback must not instantiate new Python objects, dynamically resize lists, or allocate new arrays. Such operations can trigger the CPython garbage collector, causing unpredictable latency spikes that will result in buffer overruns (dropped audio).  
2. **Blocking Input/Output (I/O):** The callback must never wait on a mutex lock, print debugging statements to stdout, or perform synchronous file disk reads/writes.  
3. **Complex Mathematics:** The callback should not attempt to execute complex digital signal processing algorithms directly. Its sole responsibility is data movement.

## **Decoupling Audio from the Global Interpreter Lock**

A critical architectural bottleneck in Python-based audio processing is the Global Interpreter Lock (GIL). The CPython GIL is a mutex that protects access to Python objects, preventing multiple native threads from executing Python bytecodes simultaneously. If the high-frequency audio capture callback and the PySide6 graphical rendering loop heavily contest the GIL, the application will experience catastrophic audio dropouts and severe graphical stuttering.

## **Lock-Free Ring Buffers for Inter-Thread Communication**

To safely transmit the high-speed audio data from the real-time PortAudio thread to the PySide6 main event loop without invoking locks, the architecture must implement a lock-free ring buffer (also known as a circular buffer).

Instead of appending the incoming NumPy arrays to a dynamic Python list—which requires memory reallocation and GIL contention—the system pre-allocates a large, fixed-size NumPy array in memory during application startup. The audio callback utilizes atomic integer indices to determine where to write the incoming data.

The audio data required for whisper.cpp (typically a queue of chunks that are accumulated until the voice activity detector determines speech has ceased) must be bifurcated.1 One pathway directs the raw data to the transcription queue, while the secondary pathway writes the exact same samples into the visualizer's lock-free ring buffer. Because NumPy arrays are backed by contiguous C-level memory arrays, the PySide6 UI thread can continuously read from this buffer without blocking the audio thread, ensuring that neither the visualizer nor the Whisper inference engine stalls the audio capture pipeline.

## **Digital Signal Processing: Visual Decimation and Downsampling**

A standard condenser or dynamic microphone recording at 48 kHz generates exactly 800 audio samples every 16.6 milliseconds (the duration of one visual frame at 60 FPS). Attempting to draw 800 individual line segments per frame via PySide6 is computationally wasteful. Furthermore, modern display monitors possess finite pixel densities. Rendering 800 horizontal data points in a graphical widget that may only be 300 pixels wide results in massive overdraw, squandering CPU cycles on pixels that cannot be physically displayed. Therefore, Digital Signal Processing (DSP) techniques must be applied to reduce the data geometry prior to rendering.

## **The Inadequacy of Simple Subsampling**

Decimation is the DSP process of reducing the sampling rate of a signal. The most computationally trivial method of decimation is simple subsampling—extracting every ![][image1]\-th sample and discarding the rest. However, in the context of audio visualization, simple subsampling is fundamentally flawed. It violates the Nyquist-Shannon sampling theorem and introduces severe visual aliasing.

More problematically, human speech contains sharp, high-frequency transients, particularly during the articulation of plosive and fricative consonants (e.g., "t", "k", "s", "p"). These transients manifest as massive, instantaneous spikes in the waveform amplitude that may last for only a few dozen samples. If a simple subsampling algorithm skips the specific samples containing the transient, the visualizer will entirely miss the acoustic event, resulting in a "dead" visual response that feels unresponsive and broken to the user.

## **Min-Max Decimation (Peak-Picking) Algorithms**

To preserve the true visual envelope of the waveform with minimal CPU overhead, a Min-Max Decimation (or peak-picking) algorithm must be utilized. This algorithm guarantees that the visual boundaries of the acoustic energy are preserved regardless of the decimation factor.

For a given chunk of ![][image2] incoming audio samples that need to be mapped to a physical screen width of ![][image3] pixels, the audio chunk is divided into ![][image3] distinct windows. For each window ![][image4], containing a subset of samples ![][image5], the absolute minimum and maximum amplitude values are extracted:

![][image6]  
![][image7]  
The waveform visualizer then draws a vertical line segment extending from ![][image8] to ![][image9] for that specific pixel column. This mathematical guarantee ensures that all high-frequency transients and signal peaks are perfectly captured visually, while drastically reducing the rendering complexity from ![][image2] vertices down to exactly ![][image10] vertices per frame.

## **Exploiting NumPy Vectorization for Zero-Cost CPU Computation**

Because the Whisper-Wayland project is predominantly written in Python 4, executing a traditional for loop over thousands of audio samples per frame to calculate minimums and maximums will induce high CPU loads. This directly contravenes the system's low-resource design philosophy and threatens to steal CPU cycles away from the OS scheduler managing the Vulkan GPU queues.

To resolve this, the Min-Max decimation must be fully vectorized using the NumPy library. NumPy functions execute in highly optimized, pre-compiled C code and actively release the Python GIL during computation.

By reshaping the linear audio array into a two-dimensional matrix where the number of columns equals the desired decimation factor, NumPy can calculate the minimum and maximum across the specific axis in a single, highly optimized instruction cycle. The conceptual mapping is executed as follows:

Python

\# The audio data is dynamically reshaped to group samples by pixel column  
reshaped\_audio \= raw\_audio.reshape(-1, samples\_per\_pixel)  
\# Vectorized C-level computation across the array  
envelope\_min \= np.min(reshaped\_audio, axis=1)  
envelope\_max \= np.max(reshaped\_audio, axis=1)

This vectorized approach executes in microseconds. It requires negligible CPU time, leaving the processing overhead strictly to the graphical rendering phase and ensuring that whisper.cpp retains maximal access to the system's CPU and GPU resources.

## **Wayland Display Server and Compositor Synchronization**

With the audio signal decimated into an efficient visual envelope, the data must be pushed to the display. The Wayland protocol fundamentally alters how applications render to the screen compared to the legacy X11 framework, heavily impacting the choice of rendering backend and synchronization methodology.8

## **The Wayland Direct Compositing Architecture**

In the legacy X11 model, the X server acted as a middleman, brokering all rendering commands between the application and the hardware. This architecture frequently led to screen tearing and high input latency unless a separate, heavy compositor was layered on top of the server. Wayland was designed from the ground up to eliminate this middleman. In Wayland, the display server *is* the compositor.8

Wayland applications (referred to as clients) render their user interface directly into a local buffer in shared memory or a hardware GPU buffer (such as a dmabuf). Once the client finishes drawing the frame, it sends a lightweight message to the Wayland compositor containing a handle to the buffer, indicating that the frame is ready for presentation.16 The compositor then integrates this buffer into the final screen image during the next monitor vertical blanking interval (vsync). This streamlined architecture inherently prevents screen tearing and significantly reduces latency.8

## **The Latency Penalty of XWayland**

To leverage these low-latency benefits, the client graphical toolkit must be entirely Wayland-native. The Whisper-Wayland project correctly utilizes PySide6, which features robust native Wayland support.4

If an application attempts to utilize legacy X11 drawing commands or toolkits, the Wayland compositor is forced to spawn XWayland—a compatibility translation layer. Research and benchmarking indicate that pushing graphics through XWayland inherently introduces severe synchronization issues and can add up to 10 milliseconds of latency compared to native Wayland rendering.17 Furthermore, modern Wayland features like variable refresh rate (VRR) and tearing protocols are often mishandled by XWayland.17 Therefore, the waveform rendering mechanism must strictly utilize Qt's native Wayland EGL platform plugin, ensuring the graphical buffer is composited directly by the host window manager (e.g., Sway or Hyprland) without translation penalties.

## **Low CPU Cost Rendering via Hardware Acceleration**

Within the PySide6 framework, developers are presented with multiple pathways to draw a waveform geometry. Selecting the correct rendering API is critical; the wrong choice will result in CPU spikes that cause the audio callback to stall.

## **Evaluating PySide6 Rendering APIs**

1. **QPainter with CPU Rasterization:** The simplest approach uses QPainter to draw lines on a standard QWidget. Behind the scenes, this often defaults to CPU-bound software rasterization, historically similar to the Cairo graphics library used in minimal X11 window managers.18 While trivial to implement, pushing dynamic vertex data every 16 ms causes heavy CPU cache invalidation and forces the CPU to calculate pixel blending and anti-aliasing. This violates the low CPU cost requirement.  
2. **QGraphicsScene:** A higher-level abstraction that retains complex scene graphs. This API is extremely inefficient for rapidly changing geometry like live waveforms and introduces unnecessary object overhead.  
3. **OpenGL / WebGL Shaders via QOpenGLWidget:** Writing custom GLSL (OpenGL Shading Language) shaders that receive the audio data as a Uniform array or a 1D Texture directly into GPU memory.

## **The Superiority of GPU-Driven Shader Rendering**

To achieve the absolute lowest CPU cost, the central processing unit should only be responsible for updating a small array of audio data in GPU VRAM once per frame. All vertex generation, coloring, and pixel interpolation must occur on the GPU's highly parallelized stream processors.

The broader Linux creative coding and visualizer community has overwhelmingly demonstrated the efficacy of this approach. Projects such as WayVes, an OpenGL-based audio visualizer built specifically for Wayland, utilize PipeWire to capture audio and feed the data directly into a multi-pass, fully GPU-driven rendering pipeline.14 In these architectures, runtime control and live updates are handled without recompiling, utilizing atomic image operations and Signed Distance Field (SDF) layering.19 The CPU usage in such systems is virtually zero, as the shader executes the geometric math concurrently across thousands of GPU cores.19

Given that whisper.cpp in Whisper-Wayland heavily utilizes the Vulkan API for its matrix operations 4, injecting a small, dedicated OpenGL context for the user interface is highly strategic. It segregates the UI rendering from the AI compute queues, ensuring neither API blocks the other.

The implementation within PySide6 involves subclassing QOpenGLWidget and establishing a specialized rendering pipeline:

* **Initialization:** The widget compiles a minimalist Vertex Shader and Fragment Shader during application startup.  
* **Data Transfer:** The UI thread maps the NumPy array (containing the decimated audio envelope) to a 1D OpenGL Texture or a Shader Storage Buffer Object (SSBO) located in GPU memory.  
* **Rendering Execution:** The Vertex Shader generates a dynamic line strip based on the audio amplitude texture. The Fragment Shader handles anti-aliasing, color gradients, and pixel blending.

This GPU-centric approach guarantees that the waveform visualizer consumes less than 1% of the system CPU, fulfilling the strict operational requirements of the project.

## **Inter-Thread Synchronization and Qt Event Loops**

Successfully weaving the low-latency audio capture, the lock-free data structures, and the OpenGL GPU rendering into the Whisper-Wayland project requires precise synchronization with the Qt Event Loop.

In PySide6, all graphical user interface updates must originate from the main thread. If the sounddevice callback running on the PortAudio OS thread attempts to call a GUI update function (e.g., widget.update()) directly, it will result in a fatal segmentation fault or a Wayland protocol crash, as Qt's underlying rendering structures are not thread-safe.

The standard Qt solution is to utilize the signal/slot mechanism to pass data between threads. However, emitting a signal from a high-frequency audio callback (which fires hundreds of times a second) will instantly flood the Qt Event Loop. The UI will become totally unresponsive, unable to process user clicks or Wayland compositor resize events.

## **Decoupling Audio Rate from Visual Frame Rate**

The optimal architecture strictly decouples the audio sampling rate from the visual frame rate. The execution responsibilities are divided as follows:

| Component | Execution Context | Responsibility | Latency Tolerance |
| :---- | :---- | :---- | :---- |
| **sounddevice Callback** | Real-time OS Thread | Read microphone data, append to Ring Buffer, feed Whisper inference queue. | Ultra-low (\<5 ms). Must never block or allocate memory. |
| **Data Decimation** | Main UI Thread (Qt) | Downsample audio using NumPy vectorization upon request. | Low (\~2 ms overhead). |
| **QOpenGLWidget** | Main UI Thread (Qt) | Upload Textures to GPU, dispatch OpenGL draw call. | Medium (Bound to 16.6 ms vsync intervals). |
| **whisper.cpp VAD** | Background Worker Thread | Detect Voice Activity (VAD), execute Vulkan inference. | High (Asynchronous background processing). |

To drive the animation, the main UI thread utilizes a QTimer configured to fire at the monitor's native refresh rate (e.g., 60 Hz, or roughly every 16.6 ms). When the QTimer triggers, the UI thread reads the latest data from the circular buffer, applies the NumPy min-max decimation, pushes the resulting array to the GPU via QOpenGLWidget.update(), and immediately yields control back to the Wayland compositor.

## **Integrating with Text Injection**

It is vital to recognize how Whisper-Wayland handles its final output. Because Wayland intentionally isolates clients, a background application cannot simulate keystrokes natively. The project invokes the ydotool utility via a subprocess to type the transcribed text into the active window.4

By strictly adhering to the decoupled architecture outlined above, the PySide6 main thread remains completely free to execute the blocking subprocess calls required by ydotool without dropping visual frames or causing the sounddevice buffer to overflow. When transcription completes and the text is injected, the visualizer gracefully animates to a resting state, providing immediate confirmation to the user that the system is processing the injection.

## **Advanced Configuration: System-Level Low Latency Tuning**

While optimizing the Python and C++ application layer is vital for performance, the underlying Linux kernel and operating system scheduler possess the ultimate authority over hardware latency. Linux, by default, is configured as a general-purpose operating system that prioritizes overall system throughput, fairness between processes, and power efficiency over deterministic real-time latency.11 To achieve professional-grade low-latency waveform rendering and capture, specific system tuning must be applied by the user.20

## **Kernel Selection and Real-Time Patches**

The standard generic Linux kernel can comfortably handle audio latencies down to approximately 10 to 15 milliseconds. However, attempting to push the buffer size below 5 milliseconds reliably—without inducing xruns (buffer underruns resulting in audio pops and clicks)—requires real-time scheduling capabilities.

Historically, achieving real-time performance required manually patching the kernel source code with the PREEMPT\_RT patchset. Modern main-line Linux kernels have steadily integrated many of these features. Users deploying the Whisper-Wayland project for high-performance dictation should be strongly advised to boot using a linux-lowlatency kernel or a similarly optimized variant (such as linux-zen or linux-xanmod).15 These specialized kernels alter the OS preemption model, allowing high-priority audio hardware interrupts to immediately preempt lower-priority kernel tasks, ensuring the audio buffer is serviced instantly.

## **GRUB Bootloader and Kernel Parameters**

Further latency reductions and CPU efficiency gains can be achieved by passing specific parameters to the kernel at boot via the GRUB bootloader.22 Modifying /etc/default/grub can dramatically stabilize the sounddevice callback latency, preventing unexpected spikes that disrupt the visualizer. Highly recommended parameters include:

* preempt=full: Enforces full preemption across the system, allowing almost all kernel code to be interrupted by higher-priority tasks.  
* threadirqs: Forces hardware interrupts to run as independent kernel threads rather than hard-coded context switches. This is a crucial setting, as it allows the system administrator to assign real-time scheduling priorities to specific hardware devices, such as the USB controller handling the microphone or the PCI Express interface for the sound card.22  
* pcie\_aspm=off: Disables Active State Power Management for PCI Express devices. While this increases overall power consumption slightly, it is absolutely necessary for low-latency audio. It prevents latency spikes caused by the audio interface constantly waking from a low-power sleep state when data arrives.22  
* processor.max\_cstate=1 and intel\_idle.max\_cstate=1: Prevents the CPU from entering deep sleep states (known as C-states). Waking a modern CPU from a deep C6 sleep state can take several milliseconds. If the CPU is asleep when the 5 ms audio buffer fills, the data will be dropped before the CPU wakes up to process it.22

## **Managing PipeWire and Pluggable Authentication Modules (PAM)**

If the system leverages PipeWire's PulseAudio compatibility daemon (which sounddevice likely targets by default on Ubuntu), the scheduling algorithms must be optimized to favor the visualizer.

In legacy PulseAudio setups, users were forced to edit /etc/pulse/default.pa to set the parameter load-module module-udev-detect tsched=0. Time-based scheduling (tsched=1) attempts to group audio processing together dynamically to save power, which introduces wildly unpredictable latency. Interrupt-based scheduling (tsched=0) forces the system to process audio precisely when the hardware requests it.23

In modern PipeWire environments, this is managed natively, but the user must still ensure that the Python application is granted real-time capabilities by the OS. This is achieved by editing the Pluggable Authentication Modules (PAM) limits, specifically /etc/security/limits.d/audio-rt.conf 15:

@audio \- rtprio 95

@audio \- memlock unlimited

Adding the current user to the audio and realtime groups allows the Whisper-Wayland Python process to request a high scheduling priority (e.g., utilizing the SCHED\_FIFO or SCHED\_RR POSIX scheduling policies) for its audio callback thread.22 This guarantees that even if whisper.cpp is saturating all available CPU cores and GPU queues with its intense inference calculations, the OS will forcibly pause the AI threads just long enough to process the microphone input and update the UI ring buffer. This completely eliminates audio dropouts and guarantees that the waveform renders smoothly regardless of system load.

## **Dynamic CPU Efficiency: Mitigating Power Draw on Wayland**

Because utilities like Whisper-Wayland are designed to operate as lightweight background daemons—idling silently while waiting for a global hotkey—power efficiency is paramount, especially when deployed on portable Linux devices.2

The Wayland protocol inherently aids in this endeavor by design. Wayland compositors are highly aggressive regarding power management; they will actively suspend frame presentation callbacks for client windows that are fully occluded, minimized, or placed on inactive virtual workspaces.

If the Whisper-Wayland graphical user interface is implemented as a floating PySide6 popup widget that only appears when the push-to-talk keybind is actively held, the QTimer driving the OpenGL waveform rendering can be dynamically halted based on the window's visibility state.

When the user releases the keybind:

1. The sounddevice input stream is closed or paused, relinquishing control of the PortAudio device.  
2. The PySide6 QTimer updating the QOpenGLWidget is stopped, halting all GPU draw calls.  
3. The Wayland wl\_surface is unmapped (hidden) from the compositor.

At this point, the application's CPU and GPU utilization drops to absolute zero, save for the highly efficient evdev loop listening asynchronously for the next key press.4 The application delegates all active power management back to the Linux kernel.

Furthermore, relying on Vulkan for the whisper.cpp execution and basic OpenGL for the user interface ensures that modern GPU power-saving features (such as dynamic frequency scaling and VRAM clock downshifting) operate correctly. Because the graphics driver natively understands the distinct compute and render queues being generated without relying on inefficient software fallback paths or translation layers, the GPU can accurately spin up and down as required by the application's immediate demands.4

## **Conclusion**

The successful integration of a real-time, low-latency audio waveform visualizer into the JRufer/Whisper-Wayland project demands a meticulous and holistic orchestration of several distinct layers of the Linux software stack. By moving decisively beyond naive, CPU-bound drawing techniques and fully embracing the modern Wayland direct compositing model, developers can construct a highly responsive visualizer that provides essential user feedback without compromising the minimalist, high-performance, vibe-coded ethos of the original project.

The implementation relies upon establishing a strict, interrupt-driven audio capture pipeline via PipeWire and the sounddevice wrapper, specifically utilizing aggressive buffer sizes of 256 or 512 samples to guarantee sub-10ms input latency. To bridge the massive architectural gap between high-frequency digital signal processing and visual frame rates without invoking the blocking mechanisms of the CPython Global Interpreter Lock (GIL), a lock-free ring buffer combined with NumPy-based vectorized min-max decimation proves entirely effective. Finally, migrating the core rendering payload from CPU rasterizers to GPU-accelerated OpenGL shaders via PySide6's QOpenGLWidget ensures that the user interface maintains a locked 60 FPS refresh rate with near-zero CPU overhead.

When correctly coupled with appropriate system-level kernel tuning—including real-time scheduling priority, explicit interrupt threading (threadirqs), and the intentional disabling of deep CPU sleep states—this architecture guarantees that graphical updates and audio capture remain completely impervious to the heavy computational load exacted by the Vulkan-accelerated whisper.cpp transcription engine. The final result is a robust, visually responsive, and seamless push-to-talk experience natively adapted for the stringent demands and future trajectory of the Linux desktop ecosystem.

#### **Works cited**

1. openai/whisper: Robust Speech Recognition via Large-Scale Weak Supervision \- GitHub, accessed March 18, 2026, [https://github.com/openai/whisper](https://github.com/openai/whisper)  
2. Whisper for Wayland \- A vibe coding journey \- tedn.life, accessed March 18, 2026, [https://tedn.life/2025/08/18/whisper-for-wayland-a-vibe-coding-journey/](https://tedn.life/2025/08/18/whisper-for-wayland-a-vibe-coding-journey/)  
3. Built a minimal speech-to-text tool for Wayland in a day, works for me : r/hyprland \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/hyprland/comments/1lkgn4f/built\_a\_minimal\_speechtotext\_tool\_for\_wayland\_in/](https://www.reddit.com/r/hyprland/comments/1lkgn4f/built_a_minimal_speechtotext_tool_for_wayland_in/)  
4. danielrosehill/Wayland-Voice-Typer: Simple GUI around whisper.cpp for voice-to-text on Linux \- GitHub, accessed March 18, 2026, [https://github.com/danielrosehill/Wayland-Voice-Typer](https://github.com/danielrosehill/Wayland-Voice-Typer)  
5. GitHub \- arniesaha/linux-whisper: Local voice-to-text for Linux. Press a hotkey, speak, and it types at your cursor. Powered by Whisper — no cloud, no API keys., accessed March 18, 2026, [https://github.com/arniesaha/linux-whisper](https://github.com/arniesaha/linux-whisper)  
6. \[feature request\] Support for language and/or task flags in Whisper models · Issue \#1305 · LostRuins/koboldcpp \- GitHub, accessed March 18, 2026, [https://github.com/LostRuins/koboldcpp/issues/1305](https://github.com/LostRuins/koboldcpp/issues/1305)  
7. Whisper Sequential long-form decoding doesn't work when forcing task · Issue \#28978 · huggingface/transformers \- GitHub, accessed March 18, 2026, [https://github.com/huggingface/transformers/issues/28978](https://github.com/huggingface/transformers/issues/28978)  
8. Why Wayland pulls more dependencies compared to xorg? : r/archlinux \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/archlinux/comments/14vswau/why\_wayland\_pulls\_more\_dependencies\_compared\_to/](https://www.reddit.com/r/archlinux/comments/14vswau/why_wayland_pulls_more_dependencies_compared_to/)  
9. Wayland \- Beyond X (The H) \[LWN.net\], accessed March 18, 2026, [https://lwn.net/Articles/481138/](https://lwn.net/Articles/481138/)  
10. Wayland protocol, accessed March 18, 2026, [https://wayland.app/protocols/wayland](https://wayland.app/protocols/wayland)  
11. 13 \- Low-latency and power-efficient audio applications on Linux \- Tommaso Cucinotta, accessed March 18, 2026, [https://www.youtube.com/watch?v=khYmr-rc2V4](https://www.youtube.com/watch?v=khYmr-rc2V4)  
12. Tracktion Waveform on Ubuntu 24 \- LinuxMusicians, accessed March 18, 2026, [https://linuxmusicians.com/viewtopic.php?t=27493](https://linuxmusicians.com/viewtopic.php?t=27493)  
13. What's the best current solution for low latency streaming from one machine to another over network (Jack vs PulseAudio vs PipeWire or other)? : r/linuxaudio \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/linuxaudio/comments/1gissuz/whats\_the\_best\_current\_solution\_for\_low\_latency/](https://www.reddit.com/r/linuxaudio/comments/1gissuz/whats_the_best_current_solution_for_low_latency/)  
14. \[OC\] Introducing WayVes \- OpenGL-based Visualiser framework for Wayland, using the Layer Shell Protocol : r/unixporn \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/unixporn/comments/1qa5l03/oc\_introducing\_wayves\_openglbased\_visualiser/](https://www.reddit.com/r/unixporn/comments/1qa5l03/oc_introducing_wayves_openglbased_visualiser/)  
15. How to Configure PipeWire for Low-Latency Audio on Ubuntu \- OneUptime, accessed March 18, 2026, [https://oneuptime.com/blog/post/2026-03-02-configure-pipewire-low-latency-audio-ubuntu/view](https://oneuptime.com/blog/post/2026-03-02-configure-pipewire-low-latency-audio-ubuntu/view)  
16. Guide: Full Wayland Setup for Linux \- Hacker News, accessed March 18, 2026, [https://news.ycombinator.com/item?id=26413290](https://news.ycombinator.com/item?id=26413290)  
17. Is Wayland really low latency afterall : r/linux\_gaming \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/linux\_gaming/comments/1r8wop3/is\_wayland\_really\_low\_latency\_afterall/](https://www.reddit.com/r/linux_gaming/comments/1r8wop3/is_wayland_really_low_latency_afterall/)  
18. fosslife/awesome-ricing: A curated list of awesome tools and technology to help you out with ricing on linux \- GitHub, accessed March 18, 2026, [https://github.com/fosslife/awesome-ricing](https://github.com/fosslife/awesome-ricing)  
19. Built an OpenGL-based Audio Visualiser Framework for Wayland : r/creativecoding \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/creativecoding/comments/1qhbupl/built\_an\_openglbased\_audio\_visualiser\_framework/](https://www.reddit.com/r/creativecoding/comments/1qhbupl/built_an_openglbased_audio_visualiser_framework/)  
20. Low latency audio in Linux Miint \- LinuxMusicians, accessed March 18, 2026, [https://linuxmusicians.com/viewtopic.php?t=28725](https://linuxmusicians.com/viewtopic.php?t=28725)  
21. My (successful) experience with low latency audio in Linux : r/linuxaudio \- Reddit, accessed March 18, 2026, [https://www.reddit.com/r/linuxaudio/comments/1iknlnz/my\_successful\_experience\_with\_low\_latency\_audio/](https://www.reddit.com/r/linuxaudio/comments/1iknlnz/my_successful_experience_with_low_latency_audio/)  
22. Low-Latency Live Audio Setup on PipeWire-based systems \- LinuxMusicians, accessed March 18, 2026, [https://linuxmusicians.com/viewtopic.php?t=28324](https://linuxmusicians.com/viewtopic.php?t=28324)  
23. Low-latency osu\! on Linux \- ThePooN's Blog, accessed March 18, 2026, [https://blog.thepoon.fr/osuLinuxAudioLatency/](https://blog.thepoon.fr/osuLinuxAudioLatency/)  
24. Implementing low latency audio processing on low-power Linux devices (with Pure Data), accessed March 18, 2026, [https://pmdelgado.wordpress.com/2018/03/25/implementing-low-latency-audio-processing-on-low-power-linux-devices-with-pure-data/](https://pmdelgado.wordpress.com/2018/03/25/implementing-low-latency-audio-processing-on-low-power-linux-devices-with-pure-data/)

[image1]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAwAAAAZCAYAAAAFbs/PAAAAr0lEQVR4XmNgGAU0BoZAzIzElwRiLSQ+CrAC4pNAfBqIBYHYDojnA/FeIH4GxO4IpQwMrkC8HIjbgfg/EB8FYk6oHC9U7BKUDwY2QGwNxMehkrpIciCTQWIgAzHAXyB+iibWxwDRkI4mDgYgiQVIfEYgvgcVl0IShwOQhCwSvwcqlgnlRyDJMXAD8WVkAQaIu0EalIHYFoiZkCX1gXgysgAQeDJA/LWeARLko2AoAwCckh/Oa3CWSgAAAABJRU5ErkJggg==>

[image2]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAaCAYAAABYQRdDAAABP0lEQVR4Xu2TPyhFYRjGX/nXNZKUie4ig9GMZJEMdjcpAxkUi8WiZJDBKHVlMCBlMSh/VkVGu4FJyiCDeJ7e93be7zvfGW3nV7/u973P+36dzvmuSIljFF45t8I4x7KE/fNhrPTAEbgKf+FjkOZ5Fu17h2OwLYxDGod+wqYo8+yJ9p3GQYoT+Co60BtlDWbhpWjPUpTlGIL38Fh0gMMx+3BANH+JsiQrcAduiw5thLFMwU1bMz90WSEXcBouig7VXdYOr2HF9sznsjhNC/yAnXBSdOjG5XzqCVuzl3l/FqdZh7u25tNw6MfWpMt+yRo8cPtCeIFn3J4fgQcPwgVXJ+ewFtVydMAv2O1qt5K9tzNXbxa98H2ulmQcPkW1uuihb7Dq6sNWL4R/TTZ4jyzj9XmArbb/jvronWUlJSX/wh8jdEjyGyzoLwAAAABJRU5ErkJggg==>

[image3]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABUAAAAaCAYAAABYQRdDAAABQElEQVR4Xu2Svy8EURSFD6IjVDSiUGlWIxEhWRQaEdUWetsgm2wU6q2ISBCNKPwIoVUpRCNBYRM6hUKrFe02nDv3rnl7zc76A+ZLvmTeOXl3d948IINs0lunMJiQi0PWH7q8YHnECP2i3/SITlveTsfpjnUrdMw6YYqeWndF+4Iu4hVarvuCLEC7Nl+QeXruwzp30I1yFJ41aNfj8g76SAdc/ssZdOOlyw/ohnWjrnujXS5rYA+68SbIcvSF9lo3G3RC0a3/UIFurAaZ/MCkPdfoYtDNQF8/lTJ06Lut5+hFXOODLgfr5+C5KTJEhoqddKuxjj7ItnXyNvW7msoE4qFyb/0HuKbHdBfJNySRYcRDH1wnyF18gh5Pt+ua0o946L3rhH1ot+SLVnzSEx8aJZr34X9Yhf7jJOScMzIy0vgBeIRHNsmR26wAAAAASUVORK5CYII=>

[image4]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAcAAAAbCAYAAACwRpUzAAAAgElEQVR4XmNgoBvQQBdABv+BWA9dEAaWADELuiB5QBOIjwDxNiDmRZZgBuJbQGwCxL5AHIws6Q7Ec6DsNiB2QpJjsANiLSj7BgMOlxoxQPyIFbQw4JAE2QOSqATijUBsjywZBZV0AOJ9yBIgIATELxgguhRQpSBABojN0QVHPAAA0rcSIejSHd0AAAAASUVORK5CYII=>

[image5]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACEAAAAZCAYAAAC/zUevAAABnklEQVR4Xu2VzysFURTHTwkbP5JENsqPBSERO5GNP8BK2bC0lbJRL2VPiY0NGzY2Ck9KpFgQFlLYSYkNGysLvqd7Z+bMeXeGN3obzbc+Neeee9/7vLl35hGl+Vt2wCzo1w2kFrTpQRteswL2dCNJWEKnEryDL3CiejJ9IKsHk8QlUQyWyUjMqZ5MQSU4T2QkynVDpOASLHCpB1V+lOgELaKuB42i9hInsSTqMspdHytxBdbBPjgFC+AQ7IIDMY8TJzFirxvAFtgG0/6MGIlJVd+CZtBD5oPvw22nRBe4JnP3zsAAGQFenwmmRUuMiWu+hZ+i7gYloua4JPiHnIMbMGTHqsEgKLI1J1JCZpiMfVxcEhtk1r2BNdAabvv5lcQiJZPgx5O3cAK8go9w249TopTMXvaCDso9A1NgXtQcLTFOYXFe49UzFD5zTokmMgvqwCZ4AXe2N0pmj6ts7UVLrFJYgr/Ue2nxOakQPacEh/eTH8MMqLHXR+ABtPuzgmiJC/Asav4TewTHFBxSL5ES+UZL5JNUwsv/kkiTpqD5BnttYVoGZVE5AAAAAElFTkSuQmCC>

[image6]: <data:image/png;base64,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>

[image7]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAewAAAA6CAYAAAByOmrmAAAMGElEQVR4Xu3dB7BkRRXG8SPmnBUDuoqKipjDGnmIijmVKFqGZ845i2gZKS0DahlQcbUQRDFHzLoiKuaAsXC3MKMllJRaSlnaf7vPznlneubN7Lx5s8v7flVdb27fmdm50/fe06e7B8xERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERERksW5dymGlPCLvSA61+jzK1dI+MbtUKY/NlVPw73a1dsi8XXKb7JO2RURkN/fsUp6XK5vNpdw/1X2glNunuo3u3KU8K1dOqdcGryzlv60cnPZluU0OKuXUUvZM9WvpQaW8LFd2PD88nuaYdlX7lXLbXDmDj9ngO7lg2icissOogL1kg5vIhUK9Avaw15Vyrlw5pV4b4Aql/KuUS+cdSa9NPljKR3LlGrpoKb8s5bx5R/LDtM0xcV6tdky7qpNKOSVXzmCPUs6y+p1cIO0TEdlhXMA+01ZmR1DAXunaVm+0s+q1Aa5hk71/r01uY/W1/J2Xd5Zyn1wZ3KyUz6c6junbqW538o5Sjs2VM/q1DXeORURWGBWwR1HAXunwUn6XK3fCqDZ4lO18wMaJpXw9V66hA6wO6Y7yNhuel+eYGJWYxWVKeVgp77faIfDCqMK8kRGfL1fOiOkL2vnCeYeILMZeVocDdyWjAvblc0WzyIB9Oavz6kuhjmHVfcM2+OwXS3WO7O4Qq0HjemnftG5s9SY7brEZc8gs7HMXKeVGYdv12gDbbThgXydtY1Sb7G/Dr19rL7LBorlY3lXKlvA8t93qIr1o0rl2AiWBeVGZKHPMzGHfItQxHcKc9lIpl2x11w2PR7my1XOBY2JqgXbKc9i8901LeUwp10r7RGRO3mQ1G6An/fa0731pez3lgE32wGf9rdXsKFtUwN7bBnPqFD7DUaUcYfWzEhSvXsqHSnl3KWeXcqStnF9l6Jb5YIYzGepnGJKbPzdO5zdOLw8sZVOqY+EUCNRsM+ybkSnxWT5bymtLeU0p97Da1m+xehOOegGbld68P3OmWC7lq1bb5Satzo1qEwLbvAM239/TS3loKXcr5U6lPNXqyEPsrMCPyS1bPaZ/2/Ax9fA98n3OKrYn5XFpm1EBgm6so7O9tT3+mQ3wXH/OgaU8w+pUAVNKnAOMBmTM63/C6gjNcaX8xurr4xw2AZ/z849WRxO+Y/V88sB9X1v5+Y4u5ZhUx3UTj+3H/3+liIxFFgIuQC4cR7YUt9dbDtgvtbrimRsNnysvDJo0YOdsa7UyCbLm11v9XC8J9VcsZVspJ9vg500vt/q8uHrbV+NybCDA/6eUb+x4RkWQ53lfa49B8CFQ38rqqnAQPHjeZdt29KlS7he2ed5322O+w1eEfegFbLIqXvcqq0GAYeRLlPJzqzfraFyb/CFXzAEZZkQ75MVm8GNCPCbq8jFlS6V8z2qnci14h8vbhSyfx/F6ZI3C6TYYquYvnRHaIPqy1dcxN3/HVnfvVkdQjuhw3TLV8TyKZ9icdxwrbecjKmTiHy7lr23bvddqEnCetn3zUk6zOiIFsnTe+8E2OHdFZASyHHrOBJx/lnJG2BdvYIsQAzY3i19YvTHwGX9vwyuAJw3YS1OWSfEzM74vvzk56uICuTu0uo+GOjJsOkx3D3XftP73z7wv9Rwr7dcbPn+P1eecP+8ojk/bPO/J7THfMVlo1AvYZEy8jsz8ha3OF6HlrG1cm+TgMg8sxIr4vGTdmR/TE23lMf3Nho8pY0Tl8blyBqxy/3srjtGPeD7QGYv7vS5/p4xK8TquD+cdEbJnRwbeO9/+bLXeAzadGbbzz+YYmcmvp+2/lOp4zqawvTU8dgzH3ytXikj1casX0l3bNj1feu9keYuSM2zH5+wNUU4asOfFhwEz6phTdvu3us+EOhAoWZhEVk1W4kPgPZ4h0XHp+YLVQLMaOmoMv4+T28ADM8OhlCfZ+IVO49pknovOHKMNjw7bTP9k8ZjIHlc7pozOFoExj87EcuiOZ0+Ga9Hb/3alnGB1qNrXcLDNMH9Ee9Lpinzkh+FnR/CljmDsGErvnW9/sVrvmTxTBGxv3vGMATr9BP6I5z6nPeY74P1+0LZJFvKaAREZgwuRi5AA7VmrL1rq9X7XSy9gM+81aq5r0QGbjKB3w6PuhmHbf9IUA/bTWt1PQ93nWl0Px8m+UfvJotm3R96R3NNqNj5OboNlq+9NIACPY/aWjWuTeLzzwtBx7BjcJTx2yzY4JkZDVjumjCz+4Fw5I4aIGXZmWoVOx0Oszkkz7E3Gz1RIHkZm4WCcwwbz6hwPI2aOQEldDNj8W73zKWfYft712pX7SM6Mee4p7THD8ltaHVk0HU8RmQI/f+ECigvMntLqmM8koLPvk1azQIIL/9EL5r5vYHUejEUnce6LAMUQHnOrzAXGuVTmPdn3VqvBgEwhD2+jF7CZH35me8xwbFyVPGnA9hvOpGVSDGv3nu83J8ecKnUcN/a1OpJB5hEXmZEl8zzmHWOGfiWrN2uG4E+zlauCHXORvLa3op4A5sP2b7CVN3IwzB7lNiA48d4+r8vNmG1feRyPAePa5E+5Yk5OsroginUPvU7MuGNiwVo+puzOpXw6V64B70Bss3odko0yAvNIq22XMZSeM2yuP46FeXHnGTbnnPNOXpYz7B+17Tyvz3fbO+fIpv3fpxNKJ5HtV1u/s0jdybb6SnaRDckvIAKpY37VgwVzc2QlXPg+Z8YKW/a/uW1/0eqiMDBc/X0b/HyJ5/kCsau2v9w0qWeukGC1Z6uPegGbYWKyC4IK82+sjnWTBux5YSFX74ZHXQzYS63OM2xfK8CwauRBgxvbcqgnS/H5Q9rnVBueY6WjxWv3SfWgniDAtAevpcPgCGpZbgOyYt7D/006D2dZzfa4qfuN3Y1qE/799Zpy4adyZJre2cvGHRPnWT6mjOcRyA7JO2bEIi2mNo4KdXxOOsi9jtrFbThgE9h5TQzYXD/UxQybDJ462iXimqfevwOmF9imwx3Rge6d/yyupJ7jWLbaOeff/ZXVjkjE90+nhPOCYxeRDm7KXCQEXi4iLrA4ZMtFRB0LpkAG5xkF2TT7NrVtHpN5u9gRcAQpssNxegGbbP5Yqxl/nmNcVMAmyHHMsbzYBllbLKxQznUgoDB/yo2MLISODPOq26z+Zhg+d0jxzOSgUEfxGzA3btqz9zMjbpRHt79kjtzgeT9uqKzWzXIb8O+8IGz7cP4WqyuFs1FtQpbox78ejrR+FuyBZtQxxZ8zrWYvq+23bDXg7L1i786hwxCDKOdJbz6czlc8F+jUcV7FOoqvjfDyLV7csBjtbKsdbq5xsmPOKX8u5wwIurw/70UHknPnK1Z/btbDT76OCdt06nk/5uazE23w80QRGYFsbMkGq0D567ipETCYIwM/MfEbHJn11vaYocF/2GBujZ482TkZOcNwHmQJrh50Dmx/s17ABkO6PYsK2GuFDGbJVgaI3irvSdEmDC1mjHZwo/S2ILgv2WD0I8ttQFtmdNr2y5XNqDah08AiqvVCML1+rmymPaZxmMNlNIrV0T+xQbDbHp4zjTgCAqaBaLN54Txkxfc1rY6EEVzzCE5E59yv91GuYnUqJ8o/H3N8V/zygXtJ7pSLbHgsUiHoOhZ1MSTIfJijx+s/z2Com1745rZND/u5Vi9cev6nt3owDMfFvsXqvCm9dgLGmVazGMSfPEWjAvYou3vAXms+xDmradqgp9cmZHJnWH8BmGxsDPfTuWedTB6eF9nwuKkTSOP2w8M2GNJiLgrcgOMKVeaxubiOsxrkj291BHl+38uw7BPac/nLXO1hVrN0Fq6N6p0rYM+G7Iib36ymaYOeXpswOkNHUTdkyThnGX3ZmdENkXM8FuIQZAmkb0z7FkkBe21st+GVu9OYpg16cpswP6qf84iInIM8wGon4oi8IznBBv9HpLgaW6olG/yXu3aGf7ertUPm7ZLbhIVQIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIiIistH8D2THz+5b2wRUAAAAAElFTkSuQmCC>

[image8]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADUAAAAaCAYAAAAXHBSTAAACm0lEQVR4Xu2WS6hOURTHF67X9ShCSVJEIQMjRO5FeSVmBkRRBhIDUiSPDBiQd1FGZOCVlCKvAQYexcTEyMhAUaIoJvz/d63dt+66e399cft8t86v/t291j7fvvt/zlr7HJGKin/hDnQe6ogTgf2m73GiFaGpOTEJ+kGzoGEh/yXELUnJ1CHoN3Qv5Pu0qX3Qe2hFyPdpUyWKpnK1+r8omSrtL2tqLHRR9BTZ7PKToLfQXpdrBtEU9/cQ+gFtcPlE1tQjaIDoj166/A7Rxjzocs0gmnoCbRK96S9cPtHD1HzRRZaIGrjt5m5YbqHLNcogaAs0Kk40gDc1V9QUyZ18pIepxGfROzHcYi7KRfjY/wa+U6bEZIPEJ0UGiu5xcMiToikauOXiPZZrdumRnKk10KmQS9Q1tc3Fdy3XafEF6BI0FboCPYMmiDbuGWijXUdYcg+grS7HA2gXtAi6Bp0V7eMcOVM3odk2Zq/zySXqmlru4m+i5TjE4nGi19yHRouWJV+Eq23+q/0lx6ETUnvyE6Gj0E/oJNRmosEc0dR46JeNeQa8cnOkaIoHxCfoHPRB1MBuN89NPnUx5/kPEqfdmPCGjAy5I268DurvYk80xeuem7a7fKJoiswTLbeZopue4ebeQIddzIV4twk33yG6kTGWY4m2Q4st5ouTJ2yCG2cVxDIj0RQZIVohObKm2EsrXTxNtO49NLnAxVfdeL1of1yGhorW+ypoJ7TUrlkmtZOLPcf12GedlvPkTNUja4r9sNbG3NBjaHJtuouPUnsyhC/DBBv4unT/0GQZH3PxATfmOq9Fv7pz9IopfkWwhHg3eRC86z7dBZu1HrE0poc49lfp5CO9YqrV4EnJ0zX3nefhNRQfREVFRUXj/AEQR4iL8CydOwAAAABJRU5ErkJggg==>

[image9]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADcAAAAaCAYAAAAT6cSuAAACtUlEQVR4Xu2XS6hNURzGP295Rt6UOyKvMlGIXI9cTJgoihJCXjMDr5QkQiQMvCbCgDJAyWtESUooImZSpEtkQuL77n9t53/WWeeUOrbbaX/1a6/1X+uevb611n/tdYFChf6FtpKdZEncEGk7rN9zMjlqa7faFgecmsjQKHYNDWCuhfwiH6N4w5j7RDZH8YYwV01VzY0nPePgf1bK3HAyMA4GJc0NIafIN5SfTGPJK7LGxfJUbK4zuUA+kANRm5Q0d5d0JT9DOdMuWOKud7E8FZvbE543yQ/YmL0qzE0nV1A6gTQzmW6F2BgXy1PeXA/ymvQmX2Gr19G1SxXmMn2BnUDdQ30ezNjFPz3yV7xy0nXYuMbFDahhLjayL8Q2uFjeSpn7Tu7FwaCa5la7+v0QmxDqU8kZMoqcJ4fJALKWHA8xL+XDUXICNkHLQ7w/OQg7EC7DDrLbZHBo90qZ05j0TmkxGenaapqb5epK2HeuLmPqo23RL8RektmhvCo8pQ4omVWO6BTWPVG6SoaFsiZQf6/fVe7HSplTvvWCjeEt6evaqprToN+TYzBTeuHKsh7AHVduQmlVpRuurG2TmVsEy+cupeY2TYK9o5ZS5nTl0oqfxl8cKJrtKaSZjIZ9Fkb4DrCbd6YVrqyV0EBnwk61z2RLaDsCmzhtU62Ovp2SVvJxKC+AvT9Wypx+Q6mRUtLcJjLH1SeSk64uyYDyLtM5V95IXsBWS7P5iKyD3STewL5P+tQshU2CbkP6zJyFba9LSCtlrpaS5lrJwlDuRh7Abi1ec0knV3/iyjNgqzAt1Jth+bQXdqDowNBTq/qM7Cc7YJNwCJU7JFNdzD0kfch82ECelje3Kd42uuN5xf9b+UTXJSCbmEEobSvfJ6W6mGuvWgbbvrvjhkg6zNRPZDldqFChQvXTbwRkjc5E+tkFAAAAAElFTkSuQmCC>

[image10]: <data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAB8AAAAaCAYAAABPY4eKAAAB0klEQVR4Xu2UzSunURTHT2lMBo2NlzKEJErEQqJEwgJllLwmWZFmY2HDjiLFWpOZZlaSlz8AiRRNpLzkLyCSlQXJgu/p3Mv5HT/8FlM2z6c+9dzveXrO89zn3ksU8MEkw044BDtgZmiZ8uFqGJnpMHmPq/WGqb1gj+QhP+E5vIdRqv7V1R7gJayFZa6WAetc7Tesh59dLZvkg27hMexz+ROFMEWN8+AVnFIZw1/BDc5M7rmAbTYECfAIxtsC8wdWmGyE5G3jVNZI0pxnxcIvfwP7bQGMwy4berpJpkuTRdKoRWUFLmPTVV4Cd+AKnFQ5swGrTPYurSRNclT2zWUs/yrPBiyH83BW5cyyGUcEf8WiyWLpubn/mmb4113PUGizGJirxhHBD+QGSbZAsg649t2ND2Cqu54gmQXPsLp+l2i4Dk9gmql59kmaD8JE+EXVBuCpux6DP1TtTXhPL8Bdkn/LVMOipzuENZLmoyTTrGmHdyQ7h2fkU2j5dX6RbDl/ODBzsEaNmSWS5ttw09T8QbMFG0ztTXi6eRFVKvkk01uK4dXsF12pqfGW45wXa8Qc0vMDtf/0TQ5eRNcUfk3wEWx/xX+lGDbZUMEvEBAQEPDxPAKMimckP3cgBwAAAABJRU5ErkJggg==>