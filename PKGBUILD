pkgname=xdg-desktop-portal-termfilechooser
pkgver=0.1.0
pkgrel=1
pkgdesc='XDG Desktop Portal file chooser backend for terminal file managers'
arch=('x86_64')
url='https://github.com/Osso/xdg-desktop-portal-termfilechooser'
license=('LicenseRef-Unknown')
depends=('gcc-libs' 'xdg-desktop-portal>=1.17.1')
makedepends=('cargo')
optdepends=(
    'kitty: default terminal command'
    'yazi: default file chooser command'
)
source=()
sha256sums=()

build() {
    cd "$startdir"
    cargo build --release --locked --offline
}

check() {
    cd "$startdir"
    cargo test --all-targets --all-features --locked --offline
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 termfilechooser.portal \
        "$pkgdir/usr/share/xdg-desktop-portal/portals/termfilechooser.portal"
    install -Dm644 org.freedesktop.impl.portal.desktop.termfilechooser.service \
        "$pkgdir/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.termfilechooser.service"
    install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
