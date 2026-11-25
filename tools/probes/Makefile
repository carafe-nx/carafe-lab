OUT ?= $(CURDIR)/build
PROBES := input gamepad sockets gdi d3d9 d3d11 d3d12 gl screen threads registry files textures shaders memory
ARCHES := i686 x86_64

LIBS_input := -ldinput -ldxguid
LIBS_gamepad := -ldinput8 -ldxguid -lsetupapi -lhid -lcfgmgr32 -ladvapi32
LIBS_gdi := -lgdi32
LIBS_d3d9 := -ld3d9
LIBS_textures := -ld3d9
LIBS_shaders := -ld3d9
LIBS_d3d11 := -ld3d11 -ldxgi -ldxguid
LIBS_d3d12 := -ld3d12 -ldxgi -ldxguid
LIBS_gl := -lopengl32 -lgdi32
LIBS_screen := -lddraw -ldxguid
LIBS_ddraw := -lddraw
LIBS_threads := -ld3d9
LIBS_registry := -ladvapi32
LIBS_files :=
LIBS_sockets := -lws2_32
EDEN_SRC ?= /src/eden
FMT_INCLUDE ?= $(firstword $(wildcard $(EDEN_SRC)/.cache/cpm/fmt/*/include))

EXES := $(foreach arch,$(ARCHES),$(foreach probe,$(PROBES),$(OUT)/$(arch)/probe-$(probe).exe)) \
	$(OUT)/i686/probe-missing.exe $(OUT)/i686/probe-ddraw.exe

.PHONY: all clean host-tests host-file-tests host-texture-tests host-regression-tests host-gl-build host-gl-tests

all: $(EXES)

host-file-tests:
	python3 -B host/test_file_lookup.py

host-tests: host-file-tests host-texture-tests host-regression-tests
	mkdir -p $(OUT)/host
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(EDEN_SRC)/src $(if $(FMT_INCLUDE),-I$(FMT_INCLUDE)) -o $(OUT)/host/page-table-test host/page-table.cpp $(EDEN_SRC)/src/common/page_table.cpp
	$(OUT)/host/page-table-test
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(EDEN_SRC)/src -o $(OUT)/host/memory-budget-test host/memory-budget.cpp
	$(OUT)/host/memory-budget-test

host-texture-tests:
	mkdir -p $(OUT)/host
	python3 -B host/extract_texture_cache.py $(EDEN_SRC) $(OUT)/host/texture-cache-methods.inc
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(OUT)/host -o $(OUT)/host/texture-cache-test host/texture-cache.cpp
	$(OUT)/host/texture-cache-test

host-regression-tests:
	mkdir -p $(OUT)/host
	python3 -B host/extract_regressions.py $(EDEN_SRC) $(OUT)/host
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(OUT)/host -I$(EDEN_SRC)/src -o $(OUT)/host/storage-binding-test host/storage-binding.cpp
	$(OUT)/host/storage-binding-test
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(OUT)/host -I$(EDEN_SRC)/src -o $(OUT)/host/nvmap-lifetime-test host/nvmap-lifetime.cpp
	$(OUT)/host/nvmap-lifetime-test
	$(CXX) -std=c++20 -O2 -Wall -Wextra -I$(OUT)/host -I$(EDEN_SRC)/src -o $(OUT)/host/indirect-storage-test host/indirect-storage.cpp
	$(OUT)/host/indirect-storage-test

host-gl-build:
	mkdir -p $(OUT)/host
	$(CC) -O2 -I$(EDEN_SRC)/externals/glad/include \
		-c $(EDEN_SRC)/externals/glad/src/glad.c -o $(OUT)/host/glad.o
	$(CXX) -std=c++20 -O2 -Wall -Wextra -ffunction-sections -fdata-sections -DFMT_HEADER_ONLY \
		-I$(EDEN_SRC)/src -I$(FMT_INCLUDE) -I$(EDEN_SRC)/externals/glad/include \
		-Wl,--gc-sections -o $(OUT)/host/opengl-staging-test host/opengl-staging.cpp \
		$(EDEN_SRC)/src/video_core/renderer_opengl/gl_staging_buffer_pool.cpp \
		$(EDEN_SRC)/src/video_core/renderer_opengl/gl_resource_manager.cpp \
		$(OUT)/host/glad.o -lGL -lX11

host-gl-tests: host-gl-build
	$(OUT)/host/opengl-staging-test

$(OUT)/i686/probe-missing.exe: missing.c missing.def
	mkdir -p $(dir $@)
	i686-w64-mingw32-dlltool -d missing.def -l $(OUT)/i686/libcarafe-missing.a
	i686-w64-mingw32-clang -O2 -Wall -Wextra -std=gnu11 -mwindows -o $@ missing.c $(OUT)/i686/libcarafe-missing.a

$(OUT)/i686/probe-%.exe: %.c probe.h
	mkdir -p $(dir $@)
	i686-w64-mingw32-clang -O2 -Wall -Wextra -std=gnu11 -mwindows -o $@ $< $(LIBS_$*) -lgdi32 -luser32

$(OUT)/x86_64/probe-%.exe: %.c probe.h
	mkdir -p $(dir $@)
	x86_64-w64-mingw32-clang -O2 -Wall -Wextra -std=gnu11 -mwindows -o $@ $< $(LIBS_$*) -lgdi32 -luser32

clean:
	rm -rf $(OUT)
