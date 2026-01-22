pkgname=xdg-desktop-portal-termfilechooser
pkgver=0.1.0
pkgrel=3
pkgdesc='XDG Desktop Portal backend for terminal file choosers (Rust)'
arch=('x86_64')
url='https://github.com/Osso/xdg-desktop-portal-termfilechooser'
license=('MIT')
depends=('xdg-desktop-portal')
makedepends=('cargo')
optdepends=(
    'kitty: default terminal'
    'yazi: default file chooser'
)
provides=('xdg-desktop-portal-impl')
source=()

build() {
    cd "$startdir"
    cargo build --release --locked
}

package() {
    cd "$startdir"
    install -Dm755 "target/release/$pkgname" "$pkgdir/usr/bin/$pkgname"
    install -Dm644 termfilechooser.portal "$pkgdir/usr/share/xdg-desktop-portal/portals/termfilechooser.portal"
    install -Dm644 org.freedesktop.impl.portal.desktop.termfilechooser.service "$pkgdir/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.termfilechooser.service"
}
