define compile
	cargo build --target ${BUILD_TRIPLET} --release --example chain --example suit  --example suit2
endef




define copy
	mkdir -p dist
	cd ../target/release/examples && cp suit suit2 chain ../../../rucimp/dist/
	cp -r ../resource dist/
endef

define cmp
	cd dist && tar -cJf rucimp_$(1)_$(2).tar.xz *
endef

main:
	$(call compile)
	$(call copy)
	$(call cmp,${BUILD_VERSION},${BUILD_TRIPLET})

cp:
	$(call copy)

rm:
	rm -rf dist