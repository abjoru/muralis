.PHONY: all rust gui test clean install

all: rust gui

rust:
	cargo build --release

gui:
	cmake -B muralis-gui/build -S muralis-gui -DCMAKE_BUILD_TYPE=Release -Wno-dev
	cmake --build muralis-gui/build

test: gui
	cargo test --workspace
	ctest --test-dir muralis-gui/build --output-on-failure

clean:
	cargo clean
	rm -rf muralis-gui/build

install: all
	install -Dm755 target/release/muralis -t $(DESTDIR)/usr/bin/
	install -Dm755 target/release/muralis-daemon -t $(DESTDIR)/usr/bin/
	install -Dm755 muralis-gui/build/muralis-gui -t $(DESTDIR)/usr/bin/
