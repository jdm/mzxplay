.PHONY: debug release

CARGO_ARGS_debug=
CARGO_ARGS_release=--release

debug:
	make app VARIANT=debug

release:
	make app VARIANT=release

app:
	EMMAKEN_CFLAGS="-s ERROR_ON_UNDEFINED_SYMBOLS=0" cargo build $(CARGO_ARGS_$(VARIANT)) --target=wasm32-unknown-emscripten
	find target/wasm32-unknown-emscripten/$(VARIANT)/deps/ -name '*.wasm' | xargs -I {} cp {} static/output/
	find target/wasm32-unknown-emscripten/$(VARIANT)/deps/ -name '*.js' | xargs -I {} cp {} static/output/
	find target/wasm32-unknown-emscripten/$(VARIANT)/deps/ -name '*.data' | xargs -I {} cp {} static/

