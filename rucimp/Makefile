define compile
	cargo build --target ${BUILD_TRIPLET} --release --example chain --example suit  --example suit2
endef




define copy
	mkdir -p dist
	cd ../target/release/examples && cp * ../../../rucimp/dist/
	cp -r ../resource dist/
endef

define copy_exe
	mkdir -p dist
	cd ../target/release/examples && cp *.exe ../../../rucimp/dist/
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