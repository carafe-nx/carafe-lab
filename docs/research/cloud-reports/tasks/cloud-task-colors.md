Ты — инженер, который находит и доказывает причину неверных цветов OpenTTD с OpenGL в эмуляторе Eden. Работай без остановок, пока не докажешь причину железно. Догадка, «вероятно» или «скорее всего» результатом не считаются. Финальный отчёт — на русском, короткими строками, без таблиц, главное в первых строках, не больше ~900 слов.

ЖЁСТКИЕ ОГРАНИЧЕНИЯ
- Ничего не пушить, не создавать PR и ветки на GitHub, не коммитить в удалённые репозитории. Результат — в финальном сообщении, патчи — unified diff прямо в тексте.
- Клонировать для чтения можно:
  - Eden: https://git.eden-emu.dev/eden-emu/eden, v0.2.1, коммит 58c1e20ee58efa3900ba616207d460886214480b; если недоступен, то зеркала на GitHub;
  - Mesa: та версия, что в журнале: «4.3 (Core Profile) Mesa 26.2.3 (git-d4a00ea0ab)». Это сборка mesa-switch: OpenGL через nvc0, Vulkan через NVK, драйвер GM20B. Если точной ветки mesa-switch нет, бери upstream Mesa 26.2 и прямо скажи, что сверял не с точной сборкой;
  - Autorun: https://github.com/autorunhq/autorun, коммит 636913ca46a3638018b3ea56995d6852db9f0165, особенно dlls/opengl32 (unix_wgl.c, thunks WOW64) и dlls/win32u/winnx_opengl.c;
  - OpenTTD 15.3: src/video/opengl.cpp и шейдеры в src/3rdparty или media/shaders.
- Игру запустить нельзя: нужны ключи консоли и ARM64-хост. Только анализ кода; компилировать куски для проверки можно.

СИСТЕМА
- Carafe собирает NSP для Switch. Внутри — рантайм Autorun: Wine + WOW64 + FEX для 32-битного кода. OpenGL игры: Wine opengl32 → Mesa nvc0 в том же процессе → GPU Switch (Maxwell GM20B).
- Стенд гоняет NSP в Eden v0.2.1: фронтенд Qt, dynarmic, бэкенд arm64, ARM64-хост, Docker/WSL2. Рендер Eden — Vulkan на lavapipe (программный, llvmpipe, LLVM 20.1.2), «Renderer.gpu_accuracy: High».
- Игра — OpenTTD 15.3 win32, драйвер видео OpenGL (по умолчанию 32bpp-anim blitter).
- В Eden наши патчи 0001–0013, их список и тексты ниже.

ЧТО ПРОВЕРЕНО И ДОКАЗАНО
1. На консоли та же NSP с OpenGL показывает правильные цвета. Пользователь проверял сам, OpenTTD с ускорением — штатная игра Autorun.
2. В Eden без ускорения (OpenTTD с ключом -v win32, рисование через GDI) цвета правильные. Кадр в этом режиме выводит компоновщик рантайма, тоже через OpenGL в Mesa и через GPU Eden. Значит, вывод на экран, Mesa и GPU Eden в целом цвета не портят.
3. В Eden с OpenGL каждый пиксель на экране = (B, 255, R) от правильного (R, G, B). Девять точек на одном и том же кадре главного меню, слева правильный (без ускорения), справа OpenGL:
   - красное окно (224,0,0) → (0,255,224);
   - кнопка Quit (252,176,48) → (48,255,252);
   - вода (28,108,124) → (124,255,28);
   - трава (32,80,4) → (12,255,64) (точки чуть смещены);
   - текст (226,18,18) → (0,255,224);
   - серое (115,117,115) → (100,255,148);
   - сиреневое поле (132,132,164) → (144,255,64);
   - заголовок (152,132,92) → (136,255,40);
   - трава (48,96,4) → (32,255,112).
   Если считать пиксель 32-битным словом BGRA (байт 0 = B, 1 = G, 2 = R, 3 = A = 255), каждый выходной канал взят из байта s XOR 2 вместо нужного байта s.
4. Описания текстур (TIC), которые видел Eden за запуск, сняты временным логом. Каждое уникальное — один раз:
   - format=0x8 (A8B8G8R8) types=UNORM swizzle=R,G,B,A type=Texture2D;
   - format=0x8 types=UNORM swizzle=B,G,R,A type=Texture1DBuffer (текстурный буфер с перестановкой BGRA);
   - format=0x1d (R8) swizzle=R,0,0,1 type=Texture1DBuffer;
   - format=0x8 swizzle=R,G,B,A type=Texture1D;
   - format=0x1d swizzle=R,0,0,1 type=Texture2D.
   Eden для текстурных буферов перестановку не применяет: BindGraphicsTextureBuffer получает только формат. Мы проверили патч, который при Texture1DBuffer + BGRA над A8B8G8R8 берёт формат B8G8R8A8 (текст ниже, «0014 отложен»). Цвета не изменились ни на один пиксель. Значит, либо этот путь не используется, либо одной перестановки мало: она не объясняет G = 255.
5. Ошибок разбора шейдеров нет: после 0013 «Invalid insn» в журнале Eden ноль, цвета те же.
6. В Eden есть PRMT только как ThrowNotImplemented. Сообщений «not implemented» в журнале нет, значит, PRMT не встречалась. I2F с selector в Eden делает BitFieldExtract(src, selector*8, 8), выглядит верно.
7. Сравнить с рендером OpenGL самого Eden не вышло: на llvmpipe он зависает на 9-й секунде, ещё до картинки.
8. До патча 0012 картинку OpenGL в Eden не было видно вообще: glReadPixels отдавал нули, экран загрузки Carafe не уходил. Поэтому мы не знаем, какими были цвета без наших патчей 0011–0013.

ЗАДАЧА
A. Найди точное место, где каналы переставляются в (B, 255, R). Пройди путь кадра OpenTTD: OpenGLBackend (как создаются и загружаются текстуры видеобуфера и anim-буфера, форматы, PBO, persistent map, шейдеры) → Wine opengl32/WOW64 → Mesa (st_pbo upload или другой путь загрузки BGRA + GL_UNSIGNED_INT_8_8_8_8_REV; что nvc0 пишет в TIC и какие инструкции Maxwell генерирует для распаковки) → Eden (TIC/формат/перестановка, texture buffer, шейдерный транслятор: какие инструкции Maxwell он переводит неверно, особенно извлечение байтов и перестановки).
B. Объясни все три признака сразу: R↔B, G → 255 (альфа) и «байт s XOR 2». Объяснение, которое покрывает только часть, не принимается.
C. Отдельно докажи, что причина не в Carafe, не в рантайме Autorun, не в Wine и не в наших патчах Eden 0011–0013. Например: на консоли тот же код Mesa и Wine даёт верные цвета, значит, разница в эмуляции; укажи, какая именно функция Eden ведёт себя не как железо.
D. Дай минимальный патч Eden (unified diff против v0.2.1 с нашими патчами) и способ проверить его за один прогон: какую строку лога или какой пиксель мы увидим. Если для доказательства нужен лог в Eden, дай патч с логом: Eden у нас пересобирается за минуту.

ЧТО СЧИТАЕТСЯ ДОКАЗАТЕЛЬСТВОМ
- Цепочка с точными ссылками file:line по всем звеньям: OpenTTD → Wine/Autorun → Mesa → Eden.
- Расчёт, который из байтов B, G, R, A выводит ровно (B, 255, R) через найденное место.
- Явный ответ, почему без ускорения и на консоли цвета правильные.

ТЕКСТЫ НАШИХ ПАТЧЕЙ EDEN
Порядок наложения 0001…0013. Кратко:
- 0001: пропуск методов на непривязанном subchannel;
- 0002: nvnflinger отпускает последний кадр отключённого слоя;
- 0003: BindObject 3D в subchannel 0 при создании 3D-контекста;
- 0004: FPCR trap enables всегда сняты (хост Snapdragon);
- 0005: пользовательские исключения как в Mesosphère, svcReturnFromException;
- 0006: ic ivau/iallu на всех ядрах;
- 0007: svcInvalidate/StoreProcessDataCache;
- 0008: код 39-битного процесса на 64 ГиБ;
- 0009: доставка data abort процессу;
- 0010: permission fault на страницах без доступа.
Полные тексты 0011–0013 и отложенного 0014 ниже.

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
Carafe bench: a texture buffer whose descriptor swaps red and blue over A8B8G8R8 is viewed as B8G8R8A8.
Vulkan buffer views have no component swizzle, so Eden dropped it, and Mesa's pixel-buffer uploads in BGRA order
(OpenTTD's OpenGL frames) came out with the wrong channels.
GPL-3.0-or-later, as the file it changes.

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
