define compile
	cargo build --target ${BUILD_TRIPLET} -F api_server -F api_client -F utils --release 
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


define cmp
	cd dist && tar -cJf ruci_cmd_$(1)_$(2).tar.xz *
endef

main:
	$(call compile)
	$(call copy)
	$(call cmp,${BUILD_VERSION},${BUILD_TRIPLET})

exe:
	$(call compile)
	$(call copy_exe)
	$(call cmp,${BUILD_VERSION},${BUILD_TRIPLET})

cp:
	$(call copy)

rm:
	rm -rf dist