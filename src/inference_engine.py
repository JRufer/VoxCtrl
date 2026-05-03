from faster_whisper import WhisperModel
import numpy as np
import threading
import queue
import time
import os
import re

# Matches standalone filler sounds, with an optional trailing comma+space
_FILLER_RE = re.compile(
    r'\b(uh+|um+|hmm+|hm+|er+|ah)\b,?\s*',
    re.IGNORECASE
)

class InferenceEngine(threading.Thread):
    def __init__(self, config, audio_queue, text_queue, realtime_text_queue):
        super().__init__(daemon=True)
        self.config = config
        self.audio_queue = audio_queue
        self.text_queue = text_queue
        self.realtime_text_queue = realtime_text_queue
        self.running = True
        self.recording = False
        self.buffer = bytearray()
        self._buffer_lock = threading.Lock()
        self.actual_device = "Unknown"
        self.actual_compute_type = "Unknown"
        
        # Setup CUDA environment for pip-installed libraries
        self._setup_cuda_env()
        
        # Load model with robust fallback
        self.model = self._load_model()
        print("Model loaded.")

    def _setup_cuda_env(self):
        """
        Arch Linux workaround: Discover and preload pip-installed CUDA/cuDNN libraries
        to satisfy ctranslate2's dependencies when system versions are mismatched.
        """
        import ctypes
        import glob
        
        # Common locations for pip-installed nvidia-* packages
        home = os.path.expanduser("~")
        python_version = f"{os.sys.version_info.major}.{os.sys.version_info.minor}"
        pip_base = os.path.join(home, ".local", "lib", f"python{python_version}", "site-packages", "nvidia")
        
        # We need to find libcublas.so.12 and libcudnn.so.9 (or whatever version is expected)
        # We'll search for these in the nvidia subfolders
        lib_search_paths = [
            os.path.join(pip_base, "cublas", "lib"),
            os.path.join(pip_base, "cudnn", "lib"),
            os.path.join(pip_base, "cuda_runtime", "lib"),
        ]
        
        # Add to LD_LIBRARY_PATH for subprocesses/ctranslate2
        existing_ld = os.environ.get("LD_LIBRARY_PATH", "")
        new_paths = []
        
        for p in lib_search_paths:
            if os.path.isdir(p):
                new_paths.append(p)
                # Preload key libraries to ensure they are available in the global symbol table
                # This often bypasses the need for the linker to find them later
                for lib_name in ["libcublas.so.*", "libcublasLt.so.*", "libcudnn.so.*"]:
                    for lib_path in glob.glob(os.path.join(p, lib_name)):
                        try:
                            # Use RTLD_GLOBAL to make symbols available to other libraries (like ctranslate2)
                            ctypes.CDLL(lib_path, mode=ctypes.RTLD_GLOBAL)
                        except Exception:
                            pass
        
        if new_paths:
            os.environ["LD_LIBRARY_PATH"] = ":".join(new_paths + ([existing_ld] if existing_ld else []))
            print(f"CUDA Environment: Injected {len(new_paths)} library paths from ~/.local")

    def _load_model(self):
        model_size = self.config.get("model_size", "base")
        device = self.config.get("device", "auto")
        compute_type = self.config.get("compute_type", "default")
        
        cache_dir = os.path.join(os.path.expanduser("~"), ".cache", "whisper-wayland")
        
        try:
            print(f"Attempting to load Whisper model '{model_size}' on '{device}'...")
            model = WhisperModel(
                model_size, 
                device=device, 
                compute_type=compute_type,
                download_root=cache_dir
            )
            
            # CRITICAL: Verify the model actually works. 
            # Often faster-whisper succeeds at creation but fails during first transcription if CUDA is broken.
            print("Verifying model operationality...")
            dummy_audio = np.zeros(16000, dtype=np.float32)
            list(model.transcribe(dummy_audio, beam_size=1, vad_filter=True))
            
            self.actual_device = device
            self.actual_compute_type = compute_type
            return model
            
        except Exception as e:
            if device != "cpu" or "libcublas" in str(e) or "cuda" in str(e).lower():
                print(f"CUDA/GPU Error detected: {e}")
                print("Forcing CPU fallback mode...")
                # Update config so we don't try GPU again next time
                self.config.set("device", "cpu")
                self.config.set("compute_type", "int8")
                self.config.save()
                
                self.actual_device = "cpu"
                self.actual_compute_type = "int8"
                return WhisperModel(
                    model_size, 
                    device="cpu", 
                    compute_type="int8",
                    download_root=cache_dir
                )
            else:
                raise e

    def _build_initial_prompt(self):
        vocab = self.config.get("custom_vocabulary", [])
        if not vocab:
            return None
        return ", ".join(vocab)

    def _postprocess(self, text):
        if self.config.get("remove_fillers", True):
            text = _FILLER_RE.sub('', text)
            text = re.sub(r'\s+', ' ', text).strip()
            # Re-capitalize first letter lost after stripping a leading filler
            if text:
                text = text[0].upper() + text[1:]
        return text

    def set_recording(self, recording):
        self.recording = recording
        if not recording:
            self.process_buffer(incremental=False)
            with self._buffer_lock:
                self.buffer.clear()

    def process_buffer(self, incremental=True):
        with self._buffer_lock:
            if not self.buffer:
                return
            # Snapshot so the audio thread can keep appending while we transcribe
            audio_bytes = bytes(self.buffer)

        audio_data = np.frombuffer(audio_bytes, dtype=np.int16).astype(np.float32) / 32768.0
        
        try:
            segments, info = self.model.transcribe(
                audio_data,
                beam_size=1,
                condition_on_previous_text=False,
                vad_filter=True,
                vad_parameters=dict(
                    min_silence_duration_ms=self.config.get("min_silence_duration_ms", 500),
                    threshold=self.config.get("vad_threshold", 0.5)
                ),
                initial_prompt=self._build_initial_prompt(),
            )

            full_text = ""
            for segment in segments:
                full_text += segment.text

            full_text = self._postprocess(full_text.strip())
            if full_text:
                if incremental:
                    self.realtime_text_queue.put(full_text)
                else:
                    self.text_queue.put(full_text)
            elif not incremental:
                # If we stop recording and there's nothing, maybe clear the UI
                self.realtime_text_queue.put("")

        except Exception as e:
            print(f"Inference error: {e}")
            if "libcublas" in str(e) or "cuda" in str(e).lower():
                print("Critical GPU error during transcription. Attempting to switch to CPU...")
                # This is harder to do mid-flight, but we can try to re-init
                self.model = self._load_model()

    def run(self):
        last_proc_time = time.time()
        while self.running:
            try:
                # Collect audio from queue
                try:
                    while True:
                        chunk = self.audio_queue.get_nowait()
                        if self.recording:
                            with self._buffer_lock:
                                self.buffer.extend(chunk)
                except queue.Empty:
                    pass

                # Process periodically if recording
                mode = self.config.get("inference_mode", "Balanced")
                interval = 0.5 if mode == "Aggressive" else 1.5
                
                if self.recording and time.time() - last_proc_time > interval:
                    self.process_buffer(incremental=True)
                    last_proc_time = time.time()
                
                time.sleep(0.05)
            except Exception as e:
                print(f"Error in inference loop: {e}")

    def stop(self):
        self.running = False
