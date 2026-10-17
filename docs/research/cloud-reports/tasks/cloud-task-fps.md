Ты — инженер, который находит и доказывает, почему OpenTTD с OpenGL в эмуляторе Eden стала рисовать 3–7 кадров в секунду вместо 30–60, и как вернуть скорость, не потеряв исправления. Работай без остановок, пока не докажешь причину железно. Догадка, «вероятно» или «скорее всего» результатом не считаются. Финальный отчёт — на русском, короткими строками, без таблиц, главное в первых строках, не больше ~900 слов.

ЖЁСТКИЕ ОГРАНИЧЕНИЯ
- Ничего не пушить, не создавать PR и ветки на GitHub, не коммитить в удалённые репозитории. Результат — в финальном сообщении, патчи — unified diff прямо в тексте.
- Клонировать для чтения можно:
  - Eden: https://git.eden-emu.dev/eden-emu/eden, v0.2.1, коммит 58c1e20ee58efa3900ba616207d460886214480b; если недоступен, то зеркала на GitHub;
  - Mesa 26.2.3 (в журнале «Mesa 26.2.3 (git-d4a00ea0ab)», сборка mesa-switch: nvc0 + NVK, GM20B; winsys nouveau для Horizon — если найдёшь);
  - Autorun: https://github.com/autorunhq/autorun, коммит 636913ca46a3638018b3ea56995d6852db9f0165;
  - OpenTTD 15.3 (src/video/opengl.cpp).
- Игру запустить нельзя. Но Mesa с nvc0 под chipset 12b через drm-shim собрать можно — прошлая облачная сессия так и делала — и прогнать путь кадра OpenTTD, считая вызовы.
- У нас локально Eden пересобирается за минуту, прогон с замером — 3–4 минуты. Если для доказательства нужны счётчики или логи в Eden, дай патч с ними: мы прогоним и вернём цифры.

СИСТЕМА
- Carafe собирает NSP для Switch. Внутри — рантайм Autorun: Wine + WOW64 + FEX. OpenGL игры: Wine opengl32 → Mesa nvc0 в том же процессе → GPU Switch.
- Стенд гоняет NSP в Eden v0.2.1: фронтенд Qt, dynarmic, бэкенд arm64, ARM64-хост (Snapdragon X), Docker/WSL2, 12 ядер. Рендер Eden — Vulkan на llvmpipe (программный, LLVM 20.1.2). use_reactive_flushing и асинхронный GPU — по умолчанию (включены). Точность GPU: в прогонах 45–65 стояла High, в прогоне 67 — по умолчанию (Medium).
- Игра — OpenTTD 15.3 win32, драйвер видео OpenGL, блиттер 40bpp-anim. Окно 1272×693.
- Наши патчи Eden 0001–0015, кратко:
  - 0001–0004: subchannel без движка, последний кадр отключённого слоя, BindObject 3D в subchannel 0, FPCR trap enables (хост Snapdragon);
  - 0005: пользовательские исключения как в Mesosphère, svcReturnFromException;
  - 0006: ic ivau/iallu на всех ядрах;
  - 0007: svcInvalidate/StoreProcessDataCache;
  - 0008: код 39-битного процесса на 64 ГиБ;
  - 0009: доставка data abort процессу;
  - 0010: permission fault на страницах без доступа;
  - 0011–0015: тексты ниже.

ЧТО ИЗМЕРЕНО
Кадры OpenGL в секунду считались по счётчику свопов рантайма (gl_frames в [PROGRESS], раз в ~10 с) после того, как игра начала рисовать. Первый запуск каждого прогона:
- прогоны 47–49 и 54, Eden с 0001–0010, без 0011: максимум 60, в среднем 18–32. Экран загрузки не уходил (чтение кадра давало нули), через 230–450 с — нехватка памяти от утечки видеопамяти;
- прогон 56, добавлен только 0011: максимум 4,4, в среднем 3,0;
- прогон 58, добавлен 0012: максимум 2,2, в среднем 1,5;
- прогон 60, добавлен 0013: максимум 8,3, в среднем 5,0;
- прогоны 62–67 (0014, 0015, временные логи): максимум 7–18, в среднем 3,4–12,4;
- прогон 67, финальный набор 0001–0015, точность GPU Medium: максимум 7,1, в среднем 4,7.
Разброс между прогонами большой (простои на замках Wine на загрузке длятся 0–200 с), поэтому одна пара цифр ничего не доказывает. Но ступенька 60 → 4 совпадает ровно с появлением 0011.

Что известно про 0011. До него Mesa на каждое освобождение буфера делала UnmapBuffer, затем FreeSpace. Eden отвечал BadValue, Mesa писала «VA teardown free … failed: 0x195c; quarantining VA range» (962 раза за запуск) и больше эти адреса не трогала. Видеопамять утекала, но адреса GPU не переиспользовались. После 0011 FreeSpace проходит, отказов ноль, куча не растёт.

ГИПОТЕЗА, КОТОРУЮ НАДО ПОДТВЕРДИТЬ ИЛИ ОПРОВЕРГНУТЬ
После 0011 Mesa переиспользует освобождённые адреса GPU. Eden при снятии привязок (gmmu Unmap, UnmapGPUMemory в texture_cache/buffer_cache) сбрасывает изображения и буферы этой области. Тогда на каждом кадре заново создаются изображения, представления, конвейеры или идут повторные выгрузки и загрузки. Другие кандидаты тоже проверь: 0012 (синхронная выгрузка при чтении процессором), ожидание заборов Mesa при освобождении буферов, quarantine VA в Mesa, сам llvmpipe.

ЗАДАЧА
A. Найди точную причину падения скорости: какой путь Eden на каждом кадре делает лишнюю работу, со ссылками file:line. Посчитай, сколько раз за кадр OpenTTD Mesa делает UnmapBuffer/FreeSpace/AllocSpace/MapBufferEx и что Eden делает на каждый вызов.
B. Отдельно оцени вклад 0012: как часто при этой нагрузке процессор читает память, изменённую GPU, и сколько стоит каждая синхронная выгрузка. Нужны ли все три части 0012 (чтение буферного кэша, текстуры без «preemtive», убранный кэш области чтения на ядро)? Какие из них на самом деле нужны для glReadPixels через промежуточный буфер, а какие можно вернуть?
C. Предложи лучший способ вернуть скорость, сохранив всё исправленное: нет утечки видеопамяти (0011), glReadPixels читает настоящий кадр (0012), правильные цвета (0014, 0015). Сравни варианты: другой 0011 (например, не сбрасывать кэши, если область тут же переиспользуется того же размера), узкий 0012 (выгрузка только при новом изменении GPU после прошлой выгрузки), настройки Eden (асинхронный GPU, use_reactive_flushing, точность), настройки llvmpipe (LP_NUM_THREADS и подобные) и ресурсы контейнера. Выбери лучший и обоснуй цифрами.
D. Дай патч Eden (unified diff против v0.2.1 с нашими патчами) и патч с временными счётчиками: что считать и раз в сколько секунд писать в журнал Eden, чтобы мы за один прогон подтвердили причину и эффект исправления. Скажи, какие числа мы должны увидеть до и после.

ЧТО СЧИТАЕТСЯ ДОКАЗАТЕЛЬСТВОМ
- Цепочка с точными ссылками file:line: OpenTTD → Mesa → Eden nvdrv → Eden texture/buffer cache, и место, где тратится время.
- Подсчёт: сколько лишних операций на кадр и почему это даёт порядок 4 кадра вместо 60 на llvmpipe.
- Явный ответ, почему без 0011 было 60 кадров и почему исправление не возвращает утечку.

ТЕКСТЫ ПАТЧЕЙ EDEN 0011–0015

===== 0011-nvdrv-unmapping-a-fixed-buffer-leaves-its-allocation.patch =====
Carafe bench: unmapping a fixed buffer also removes it from the allocation it was mapped into, so a later
FreeSpace of that allocation succeeds as on the hardware. Mesa unmaps each buffer before it frees its space;
Eden then answered BadValue, the space was never freed and the process ran out of memory.
GPL-3.0-or-later, as the file it changes.

--- a/src/core/hle/service/nvdrv/devices/nvhost_as_gpu.cpp
+++ b/src/core/hle/service/nvdrv/devices/nvhost_as_gpu.cpp
@@ -5,7 +5,9 @@
 // SPDX-FileCopyrightText: 2021 Skyline Team and Contributors
 // SPDX-License-Identifier: GPL-3.0-or-later
 
+#include <algorithm>
 #include <cstring>
+#include <iterator>
 #include <utility>
 
 #include "common/alignment.h"
@@ -395,6 +397,9 @@
             auto& allocator{mapping.big_page ? *vm.big_page_allocator : *vm.small_page_allocator};
             u32 page_size_bits{mapping.big_page ? vm.big_page_size_bits : VM::PAGE_SIZE_BITS};
             allocator.Free(u32(mapping.offset >> page_size_bits), u32(mapping.size >> page_size_bits));
+        } else if (auto alloc = allocation_map.upper_bound(params.offset); alloc != allocation_map.begin()) {
+            auto& mappings = std::prev(alloc)->second.mappings;
+            mappings.erase(std::remove(mappings.begin(), mappings.end(), params.offset), mappings.end());
         }
 
         // Sparse mappings shouldn't be fully unmapped, just returned to their sparse state

===== 0012-gpu-cpu-reads-download-gpu-modified-memory-every-time.patch =====
Carafe bench: a CPU read of memory the GPU has modified downloads it every time, as the hardware shows the
CPU what the GPU wrote. Eden skipped the download for buffers, for an image read before, and for any read
inside the area a core read last, so glReadPixels through a staging buffer returned stale zeros.
GPL-3.0-or-later, as the files it changes.

--- a/src/core/memory.cpp
+++ b/src/core/memory.cpp
@@ -754,11 +754,6 @@
         const size_t core = system.GetCurrentHostThreadID();
         auto& current_area = rasterizer_read_areas[core];
         gpu_device_memory->ApplyOpOnPointer(p, scratch_buffers[core], [&](DAddr address) {
-            const DAddr end_address = address + size;
-            if (current_area.start_address <= address && end_address <= current_area.end_address)
-                [[likely]] {
-                return;
-            }
             current_area = system.GPU().OnCPURead(address, size);
         });
     }
--- a/src/video_core/texture_cache/texture_cache.h
+++ b/src/video_core/texture_cache/texture_cache.h
@@ -676,7 +676,7 @@
             auto& image_view = slot_image_views[image_view_id];
             image_view.flags |= ImageViewFlagBits::PreemtiveDownload;
         }
-        area->preemtive &= image.info.forced_flushed;
+        area->preemtive = false;
         image.info.forced_flushed = true;
     });
     return area;
--- a/src/video_core/renderer_vulkan/vk_rasterizer.cpp
+++ b/src/video_core/renderer_vulkan/vk_rasterizer.cpp
@@ -671,6 +671,12 @@
         .end_address = Common::AlignUp(addr + size, Core::DEVICE_PAGESIZE),
         .preemtive = true,
     };
+    {
+        std::scoped_lock lock{buffer_cache.mutex};
+        if (buffer_cache.IsRegionGpuModified(addr, size)) {
+            new_area.preemtive = false;
+        }
+    }
     return new_area;
 }
 

===== 0013-shader-cache-slow-path-starts-after-the-program-header.patch =====
Carafe bench: when a shader has no self-branch after its end, as with Mesa's nvc0, the control flow graph that
finds its size starts after the program header, as the translation does. Eden decoded the header as code, so
the size and hash were wrong and draws could get another shader with the same start.
GPL-3.0-or-later, as the file it changes.

--- a/src/video_core/shader_cache.cpp
+++ b/src/video_core/shader_cache.cpp
@@ -236,10 +236,13 @@
     } else {
         // Slow path, not really hit on commercial games
         // Build a control flow graph to get the real shader size
+        const u32 header_size{env.ShaderStage() == Shader::Stage::Compute
+                                  ? 0U
+                                  : static_cast<u32>(sizeof(Shader::ProgramHeader))};
         Shader::ObjectPool<Shader::Maxwell::Flow::Block> flow_block;
-        Shader::Maxwell::Flow::CFG cfg{env, flow_block, env.StartAddress()};
+        Shader::Maxwell::Flow::CFG cfg{env, flow_block, env.StartAddress() + header_size};
         info->unique_hash = env.CalculateHash();
-        info->size_bytes = env.ReadSizeBytes();
+        info->size_bytes = env.ReadSizeBytes() + header_size;
     }
     const size_t size_bytes{info->size_bytes};
     const ShaderInfo* const result{info.get()};

===== 0014-texture-buffer-bgra-swizzle-becomes-a-bgra-format.patch =====
Carafe bench: a texture buffer whose descriptor swaps red and blue over A8B8G8R8 is viewed as B8G8R8A8. The
buffer view takes its format from the view, not the image, and uses B8G8R8A8 when the device can read texel
buffers in it. Vulkan buffer views have no component swizzle, so Eden dropped it, and Mesa's pixel-buffer uploads
in BGRA order (OpenTTD's OpenGL video buffer) came out with red and blue swapped.
GPL-3.0-or-later, as the files it changes.

--- a/src/video_core/texture_cache/image_view_info.cpp
+++ b/src/video_core/texture_cache/image_view_info.cpp
@@ -35,6 +35,17 @@
       y_source{CastSwizzle(config.y_source)},
       z_source{CastSwizzle(config.z_source)},
       w_source{CastSwizzle(config.w_source)} {
+    const bool bgra_swizzle = x_source == CastSwizzle(SwizzleSource::B) &&
+                              y_source == CastSwizzle(SwizzleSource::G) &&
+                              z_source == CastSwizzle(SwizzleSource::R) &&
+                              w_source == CastSwizzle(SwizzleSource::A);
+    if (config.texture_type == TextureType::Texture1DBuffer && bgra_swizzle &&
+        (format == PixelFormat::A8B8G8R8_UNORM || format == PixelFormat::A8B8G8R8_SRGB)) {
+        format = format == PixelFormat::A8B8G8R8_UNORM ? PixelFormat::B8G8R8A8_UNORM
+                                                       : PixelFormat::B8G8R8A8_SRGB;
+        x_source = CastSwizzle(SwizzleSource::R);
+        z_source = CastSwizzle(SwizzleSource::B);
+    }
     range.base = SubresourceBase{
         .level = static_cast<s32>(config.res_min_mip_level),
         .layer = base_layer,
--- a/src/video_core/texture_cache/image_view_base.cpp
+++ b/src/video_core/texture_cache/image_view_base.cpp
@@ -37,7 +37,7 @@
 }
 
 ImageViewBase::ImageViewBase(const ImageInfo& info, const ImageViewInfo& view_info, GPUVAddr addr)
-    : image_id{NULL_IMAGE_ID}, gpu_addr{addr}, format{info.format}, type{ImageViewType::Buffer},
+    : image_id{NULL_IMAGE_ID}, gpu_addr{addr}, format{view_info.format}, type{ImageViewType::Buffer},
       size{
           .width = info.size.width,
           .height = 1,
--- a/src/video_core/renderer_vulkan/vk_buffer_cache.cpp
+++ b/src/video_core/renderer_vulkan/vk_buffer_cache.cpp
@@ -116,6 +116,14 @@
     if (it != views.end()) {
         return *it->handle;
     }
+    VkFormat view_format = MaxwellToVK::SurfaceFormat(*device, FormatType::Buffer, false, format).format;
+    const VkFormat bgra_format = format == VideoCore::Surface::PixelFormat::B8G8R8A8_UNORM ? VK_FORMAT_B8G8R8A8_UNORM
+                                 : format == VideoCore::Surface::PixelFormat::B8G8R8A8_SRGB ? VK_FORMAT_B8G8R8A8_SRGB
+                                                                                            : VK_FORMAT_UNDEFINED;
+    if (bgra_format != VK_FORMAT_UNDEFINED &&
+        device->IsFormatSupported(bgra_format, VK_FORMAT_FEATURE_UNIFORM_TEXEL_BUFFER_BIT, FormatType::Buffer)) {
+        view_format = bgra_format;
+    }
     views.push_back({
         .offset = offset,
         .size = size,
@@ -125,7 +133,7 @@
             .pNext = nullptr,
             .flags = 0,
             .buffer = *buffer,
-            .format = MaxwellToVK::SurfaceFormat(*device, FormatType::Buffer, false, format).format,
+            .format = view_format,
             .offset = offset,
             .range = size,
         }),

===== 0015-sw-blitter-packed-8888-components-start-at-the-low-byte.patch =====
Carafe bench: the software blitter lists A8R8G8B8 and A8B8G8R8 components from the low byte up, as its converter
reads them and as the rest of Eden lays these formats out (B or R in byte 0). They were listed from the high byte,
so a 2D copy from A8R8G8B8 to A8B8G8R8, which Mesa uses to upload OpenTTD's palette, turned (R, G, B) into
(B, 255, R).
GPL-3.0-or-later, as the file it changes.

--- a/src/video_core/engines/sw_blitter/converter.cpp
+++ b/src/video_core/engines/sw_blitter/converter.cpp
@@ -283,7 +283,7 @@
         ComponentType::UNORM, ComponentType::UNORM, ComponentType::UNORM, ComponentType::UNORM};
     static constexpr std::array<size_t, num_components> component_sizes = {8, 8, 8, 8};
     static constexpr std::array<Swizzle, num_components> component_swizzle = {
-        Swizzle::A, Swizzle::R, Swizzle::G, Swizzle::B};
+        Swizzle::B, Swizzle::G, Swizzle::R, Swizzle::A};
 };
 
 struct A8R8G8B8_SRGBTraits {
@@ -328,7 +328,7 @@
         ComponentType::UNORM, ComponentType::UNORM, ComponentType::UNORM, ComponentType::UNORM};
     static constexpr std::array<size_t, num_components> component_sizes = {8, 8, 8, 8};
     static constexpr std::array<Swizzle, num_components> component_swizzle = {
-        Swizzle::A, Swizzle::B, Swizzle::G, Swizzle::R};
+        Swizzle::R, Swizzle::G, Swizzle::B, Swizzle::A};
 };
 
 struct A8B8G8R8_SRGBTraits {
