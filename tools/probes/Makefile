OUT ?= $(CURDIR)/build
PROBES := input gdi d3d9 d3d11 d3d12 gl
ARCHES := i686 x86_64

LIBS_input :=
LIBS_gdi := -lgdi32
LIBS_d3d9 := -ld3d9
LIBS_d3d11 := -ld3d11 -ldxgi -ldxguid
LIBS_d3d12 := -ld3d12 -ldxgi -ldxguid
LIBS_gl := -lopengl32 -lgdi32

EXES := $(foreach arch,$(ARCHES),$(foreach probe,$(PROBES),$(OUT)/$(arch)/probe-$(probe).exe))

.PHONY: all clean

all: $(EXES)

$(OUT)/i686/probe-%.exe: %.c probe.h
	mkdir -p $(dir $@)
	i686-w64-mingw32-clang -O2 -Wall -Wextra -std=gnu11 -mwindows -o $@ $< $(LIBS_$*) -lgdi32 -luser32

$(OUT)/x86_64/probe-%.exe: %.c probe.h
	mkdir -p $(dir $@)
	x86_64-w64-mingw32-clang -O2 -Wall -Wextra -std=gnu11 -mwindows -o $@ $< $(LIBS_$*) -lgdi32 -luser32

clean:
	rm -rf $(OUT)
