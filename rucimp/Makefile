# Common variables
DIST_DIR := dist
TARGET_DIR := ../target/${BUILD_TRIPLET}/release/examples
RESOURCE_DIR := ../resource

# run like : make BUILD_TRIPLET=aarch64-apple-darwin

# Common commands
define make_dist
	mkdir -p ${DIST_DIR}
	cd ${TARGET_DIR} && cp $(1) ../../../../rucimp/${DIST_DIR}/
	cp -r ${RESOURCE_DIR} ${DIST_DIR}/
endef

define compile
	cargo build --target ${BUILD_TRIPLET} -F lua --release --example lua
endef

define package
	cd ${DIST_DIR} && tar -cJf rucimp_${BUILD_VERSION}_${BUILD_TRIPLET}.tar.xz *
endef

# Targets
main:
	$(call compile)
	$(call make_dist,lua)
	$(call package)

exe:
	$(call compile)
	$(call make_dist,lua.exe)
	$(call package)

cp:
	$(call make_dist,lua)

rm:
	rm -rf ${DIST_DIR}