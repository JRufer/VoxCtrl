# Maintainer: Your Name <youremail@example.com>
pkgname=voxctl
pkgver=0.1.0
pkgrel=1
pkgdesc="Native, on-device voice-to-text pipeline for Arch Linux (Wayland/X11)"
arch=('any')
url="https://github.com/jrufer/voxctl"
license=('MIT')
depends=('python' 'python-pyqt6' 'python-evdev' 'portaudio' 'wl-clipboard')
optdepends=('python-faster-whisper: for the transcription engine'
            'python-websockets: for WebSocket delivery support'
            'python-mcp: for MCP server feature'
            'python-atspi: for AT-SPI2 focus tracking and direct text insertion')
source=("voxctl.desktop" "99-voxctl.rules")
sha256sums=('SKIP' 'SKIP')

package() {
    mkdir -p "$pkgdir/opt/voxctl"
    cp -r "$srcdir/../src/"* "$pkgdir/opt/voxctl/"
    
    # Install desktop entry
    install -Dm644 "$srcdir/voxctl.desktop" "$pkgdir/usr/share/applications/voxctl.desktop"
    
    # Install udev rules
    install -Dm644 "$srcdir/99-voxctl.rules" "$pkgdir/usr/lib/udev/rules.d/99-voxctl.rules"
}
