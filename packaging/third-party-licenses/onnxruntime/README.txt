ONNX Runtime
============

Copyright (c) Microsoft Corporation. MIT License (LICENSE), with the notices
for the third-party code it contains in ThirdPartyNotices.txt. Taken from
Microsoft's onnxruntime 1.31.1 release.

VoxCtrl links ONNX Runtime into every build that has the Moonshine, Parakeet
or Inflect-Micro engines. Linux builds made for Ubuntu 22.04 and Debian 12
ship Microsoft's libonnxruntime.so.1 next to the app instead
(scripts/fetch-onnxruntime-linux.sh); those packages install these texts in
/usr/share/doc/voxctrl/onnxruntime.
