!macro NSIS_HOOK_POSTINSTALL
  # Copy the CUDA DLLs from the resources folder to the main application folder next to voxctrl.exe
  IfFileExists "$INSTDIR\resources\cudart64_12.dll" 0 +3
    CopyFiles "$INSTDIR\resources\cudart64_12.dll" "$INSTDIR\cudart64_12.dll"
    Delete "$INSTDIR\resources\cudart64_12.dll"
  
  IfFileExists "$INSTDIR\resources\cublas64_12.dll" 0 +3
    CopyFiles "$INSTDIR\resources\cublas64_12.dll" "$INSTDIR\cublas64_12.dll"
    Delete "$INSTDIR\resources\cublas64_12.dll"

  IfFileExists "$INSTDIR\resources\cublasLt64_12.dll" 0 +3
    CopyFiles "$INSTDIR\resources\cublasLt64_12.dll" "$INSTDIR\cublasLt64_12.dll"
    Delete "$INSTDIR\resources\cublasLt64_12.dll"
!macroend
