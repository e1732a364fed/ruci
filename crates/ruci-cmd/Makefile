define compile
	export RUSTFLAGS="-Ctarget-feature=+crt-static"
	cargo build --target ${BUILD_TRIPLET} -F steganography -F lua -F file_server -F api_server -F api_client -F utils -F quinn -F use-native-tls -F tun -F smoltcp ${EXTRA} --release
endef

define compile_win_gnu
	powershell -c { $$Env:RUSTFLAGS="-Clink-self-contained=yes -Ctarget-feature=+crt-static"; cargo build -F steganography -F lua -F file_server -F api_server -F api_client -F utils -F quinn -F use-native-tls -F tun -F smoltcp ${EXTRA} --release }
endef

define compile_cross
	export RUSTFLAGS="-Ctarget-feature=+crt-static"
	cargo build --target ${BUILD_TRIPLET} -F steganography -F lua54 -F file_server -F api_server -F api_client -F utils -F quinn -F native-tls-vendored -F tun -F smoltcp ${EXTRA} --release
endef


define copy
	mkdir -p dist
	cd ../../target/${BUILD_TRIPLET}/release && cp ruci-cmd ../../../crates/ruci-cmd/dist/
	cp -r ../../resource dist/
endef

define copy_exe
	mkdir -p dist
	cd ../../target/${BUILD_TRIPLET}/release && cp ruci-cmd.exe ../../../crates/ruci-cmd/dist/
	cp -r ../../resource dist/
endef


define xz
	cd dist && tar -cJf ruci_cmd_$(1)_$(2).tar.xz *
endef

main:
	$(call compile)
	$(call copy)
	$(call xz,${BUILD_VERSION},${BUILD_TRIPLET})

exe:
	$(call compile)
	$(call copy_exe)
	$(call xz,${BUILD_VERSION},${BUILD_TRIPLET})

win_gnu:
	$(call compile_win_gnu)
	$(call copy_exe)
	$(call xz,${BUILD_VERSION},${BUILD_TRIPLET})

vendor:
	$(call compile_cross)
	$(call copy)
	$(call xz,${BUILD_VERSION},${BUILD_TRIPLET})

cp:
	$(call copy)

rm:
	rm -rf dist