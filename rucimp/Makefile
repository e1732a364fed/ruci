define compile
	cargo build --target ${BUILD_TRIPLET}  -F lua --release --example chain --example suit
endef

define copy
	mkdir -p dist
	cd ../target/${BUILD_TRIPLET}/release/examples && cp suit chain ../../../../rucimp/dist/
	cp -r ../resource dist/
endef

define copy_exe
	mkdir -p dist
	cd ../target/${BUILD_TRIPLET}/release/examples && cp suit.exe chain.exe ../../../../rucimp/dist/
	cp -r ../resource dist/
endef


define cmp
	cd dist && tar -cJf rucimp_$(1)_$(2).tar.xz *
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