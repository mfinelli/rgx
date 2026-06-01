PREFIX := /usr/local
DESTDIR :=

GREP := grep
ifeq ($(shell uname), Darwin)
	GREP := ggrep
endif

VERSION ?= $(shell $(GREP) -m1 '^version ' Cargo.toml | awk -F\" '{print $$2}')
TODAY ?= $(shell date +%Y-%m-%d)

all: target/release/rgx target/release/rgx.bash target/release/rgx.fish \
	target/release/rgx.zsh

clean:
	rm -rf target

target/release/rgx: $(wildcard src/*.rs)
	cargo build --frozen --release

target/release/rgx.bash: target/release/rgx
	./target/release/rgx completions bash > $@

target/release/rgx.fish: target/release/rgx
	./target/release/rgx completions fish > $@

target/release/rgx.zsh: target/release/rgx
	./target/release/rgx completions zsh > $@

install: all
	install -Dm0755 target/release/rgx "$(DESTDIR)$(PREFIX)/bin/rgx"
	install -Dm0644 README.md \
		"$(DESTDIR)$(PREFIX)/share/doc/rgx/README.md"
	install -Dm0644 target/release/rgx.bash \
		"$(DESTDIR)$(PREFIX)/share/bash-completion/completions/rgx"
	install -Dm0644 target/release/rgx.fish \
		"$(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/rgx.fish"
	install -Dm0644 target/release/rgx.zsh \
		"$(DESTDIR)$(PREFIX)/share/zsh/site-functions/_rgx"

uninstall:
	rm -rf \
		"$(DESTDIR)$(PREFIX)/bin/rgx" \
		"$(DESTDIR)$(PREFIX)/share/doc/rgx" \
		"$(DESTDIR)$(PREFIX)/share/bash-completion/completions/completions/rgx" \
		"$(DESTDIR)$(PREFIX)/share/fish/vendor_completions.d/rgx.fish" \
		"$(DESTDIR)$(PREFIX)/share/doc/zsh/site-functions/_rgx"

.PHONY: all clean install uninstall
