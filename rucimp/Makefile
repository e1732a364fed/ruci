define compile
	cargo build --target ${BUILD_TRIPLET} --release --example chain --example suit  --example suit2
endef




define copy
	mkdir -p dist
	cd ../target/${BUILD_TRIPLET}/release/examples && cp suit suit2 chain ../../../rucimp/dist/
	cp -r ../resource dist/
endef

define copy_exe
	mkdir -p dist
	cd ../target/${BUILD_TRIPLET}/release/examples && cp suit.exe suit2.exe chain.exe ../../../rucimp/dist/
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