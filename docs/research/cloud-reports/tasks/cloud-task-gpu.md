Ты — инженер, который переводит эмулятор Eden в Docker на WSL2 с программного рендера на видеокарту ПК (Qualcomm Adreno X1-85 через /dev/dxg), чтобы OpenTTD с OpenGL в Eden рисовал быстро. Работай без остановок, пока не докажешь причину каждого сбоя и не дашь рабочий путь. Догадка, «вероятно» или «скорее всего» результатом не считаются. Финальный отчёт — на русском, короткими строками, без таблиц, главное в первых строках, не больше ~900 слов. В первых строках — только доказанное; недоказанные звенья — отдельным списком «не доказано».

ЖЁСТКИЕ ОГРАНИЧЕНИЯ
- Ничего не пушить, не создавать PR и ветки на GitHub, не коммитить в удалённые репозитории. Результат — в финальном сообщении, правки — unified diff прямо в тексте.
- У себя в облаке скачивай что нужно: пакеты Ubuntu 24.04 arm64 и их отладочные символы (ddebs.ubuntu.com, Launchpad), исходники Mesa, Eden, PPA.
- Клонировать для чтения: Eden https://git.eden-emu.dev/eden-emu/eden, v0.2.1, коммит 58c1e20ee58efa3900ba616207d460886214480b (если недоступен — зеркала на GitHub); Mesa (gitlab.freedesktop.org/mesa/mesa), тег, из которого собран пакет 25.2.8-0ubuntu0.24.04.4, и main.
- Видеокарты Qualcomm и WSL у тебя нет. Если для доказательства нужен прогон у нас, дай точные команды и что мы должны увидеть. Прогон у нас — 5 минут, пересборка образа — несколько минут; скачивания у нас согласуются отдельно, поэтому укажи их объём.

СИСТЕМА
- Хост: Windows 11 ARM64, Snapdragon X (12 ядер), Adreno X1-85, драйвер Qualcomm qcdx8380 (в WSL: /usr/lib/wsl/drivers/qcdx8380.inf_arm64_34a2c09c0e5b25ac/, там libqcdx12arm64wslum.so и qcgpuarm64compilercore.so). Docker Desktop на WSL2.
- Контейнер: ubuntu:24.04 arm64. Пакеты Mesa из Ubuntu: 25.2.8-0ubuntu0.24.04.4 (mesa-vulkan-drivers, libgl1-mesa-dri, libgallium-25.2.8-0ubuntu0.24.04.4.so). В /usr/share/vulkan/icd.d нет dzn_icd*.json: Ubuntu не собирает Dozen, Vulkan на видеокарте недоступен. Поэтому на видеокарте Eden идёт через OpenGL на Mesa d3d12.
- Контейнер получает `--device /dev/dxg -v /usr/lib/wsl:/usr/lib/wsl:ro`. Окружение режима видеокарты:
  LD_LIBRARY_PATH=/usr/lib/wsl/lib; GALLIUM_DRIVER=d3d12; MESA_D3D12_DEFAULT_ADAPTER_NAME пустой. Экран — Xvfb :1 1280x720x24, Eden рисует в окно X через GLX.
- В /usr/lib/wsl/lib: libd3d12.so, libd3d12core.so, libdxcore.so.
- Eden v0.2.1, собран из исходников (gcc-14, RelWithDebInfo) с нашими патчами 0001–0015 (субканалы, исключения процесса, кэш данных, nvdrv, шейдерный кэш, форматы); ни один не трогает renderer_opengl. Фронтенд Qt (на стенде) и eden-cli (для отладки под gdb). Бэкенд CPU — dynarmic arm64.
- Гость: NSP Carafe — Wine + WOW64 + FEX, внутри Mesa nvc0 рисует на эмулируемый Maxwell (GM20B). Игра — OpenTTD 15.3 win32, видеодрайвер OpenGL.
- Программный режим (сейчас на стенде): LIBGL_ALWAYS_SOFTWARE=1, Vulkan на lavapipe (LLVM 20), Eden backend Vulkan. OpenTTD доходит до меню, ~37 кадров/с в лучшем случае; рендер упирается в процессор.

ЧТО ИЗМЕРЕНО 12 ОКТЯБРЯ (режим видеокарты)
1. Qualcomm подменяет обработчик SIGSEGV. Точка останова на sigaction(SIGSEGV) сработала 59 раз:
   #0 sigaction () libc.so.6
   #1 llvm::sys::OverrideSignalHandler::overrideSignal() qcgpuarm64compilercore.so
   #2 llvm::sys::OverrideSignalHandler::OverrideSignalHandler(sigType) qcgpuarm64compilercore.so
   #3 QGPUCompiler::CreateD3D12Program(CompileData*) qcgpuarm64compilercore.so
   #4 QGPUCompiler::CreateProgram(CompileData*)
   #5 QGPUCompiler::Compile(Context*, CompilerOptionsList*)
   #6 QNGCCGetDXShaderModelSupport(...)
   #7 DxDevice::Init(DxAdapter*, DxCreateDeviceCommon const*) libqcdx12arm64wslum.so
   и парные restoreOldHandler() из ~OverrideSignalHandler(). Пока компилятор работает, ошибки fastmem dynarmic в потоках CPU уходят не в обработчик Eden, и Eden падает. С cpu_accuracy=Debugging и cpuopt_fastmem=false этого падения нет.
2. Рекурсия в Mesa d3d12. С обходом из п.1 игра стартует, GL_RENDERER «D3D12 (Qualcomm(R) Adreno(TM) X1-85 GPU)», GL_VERSION «4.6 (Compatibility Profile) Mesa 25.2.8-0ubuntu0.24.04.4». Через ~2 с после первого CreateGraphicsPipeline гостя (60,3 с от старта; до этого есть предупреждение «GL_ARB_seamless_cubemap_per_texture is required») поток Eden «GPU» падает с SIGSEGV от переполнения стека. Одинаково с Renderer.backend OpenGL_GLSL и OpenGL_SPIRV. ulimit -s 256 МБ не помогает — рекурсия бесконечная. Символов у пакета нет. Журнал gdb (адреса абсолютные, база библиотеки не записана; страница 4 КиБ, младшие 12 бит адресов совпадают со смещениями в файле, разности адресов точные):

Thread 32 "GPU" received signal SIGSEGV, Segmentation fault.
0x0000fffed5f48404 in ?? () from /lib/aarch64-linux-gnu/libgallium-25.2.8-0ubuntu0.24.04.4.so
   0xfffed5f483ec:	str	x1, [sp, #2552]
   0xfffed5f483f0:	mov	x1, #0x0
   0xfffed5f483f4:	ldr	w27, [x20]
   0xfffed5f483f8:	mov	w0, #0xffffffff
   0xfffed5f483fc:	stp	q0, q0, [x21]
   0xfffed5f48400:	mov	x23, x3
=> 0xfffed5f48404:	str	x25, [sp, #8]
   0xfffed5f48408:	str	q0, [x21, #32]
   0xfffed5f4840c:	str	w0, [sp, #44]
   0xfffed5f48410:	str	xzr, [x21, #48]
   0xfffed5f48414:	str	w27, [sp, #548]
   0xfffed5f48418:	cmp	w27, #0x3
x0 0xffffffff  x1 0x0  x2 0x0  x3 0x0  x30 0x1efffed5f4af70 (PAC)  sp 0xfffd517bfe50
#0  0x0000fffed5f48404
#1  0x0000fffed5f4af70
#2  0x0000fffed5fad18c
#3  0x0000fffed5f83730
#4  0x0000fffed5f837f8
#5  0x0000fffed5f83b58
#6  0x0000fffed5f846d8
#7  0x0000fffed5f82000
#8  0x0000fffed5f8373c
#9  0x0000fffed5f837f8
#10 0x0000fffed5f83b58
#11 0x0000fffed5f846d8
#12 0x0000fffed5f82000
#13 0x0000fffed5f8373c
... цикл 837f8 → 83b58 → 846d8 → 82000 → 8373c повторяется до #39 и дальше.

ЗАДАЧА
A. Восстанови символы стека п.2. Скачай libgallium-25.2.8-0ubuntu0.24.04.4.so (arm64) и его dbgsym. Найди базу: по сигнатуре инструкций у #0 и по тому, что каждый адрес возврата стоит сразу после bl/blr. Назови функции цикла и #0–#3 с file:line в исходниках Mesa этой версии.
B. Найди причину рекурсии: какой вызов GL от Eden её запускает (renderer_opengl, file:line), почему цикл не кончается (баг Mesa d3d12 / NIR / gallium, или неверное использование GL Eden-ом). Проверь историю Mesa после 25.2.8: исправлено ли это и в какой версии (коммит).
C. Сравни способы получить рендер на видеокарте и выбери лучший с обоснованием:
   - другая Mesa в контейнере для d3d12 (PPA kisak-mesa/oibaf, если есть arm64 для noble; или своя сборка Mesa — какие опции meson);
   - Vulkan через Dozen (своя сборка Mesa с -Dvulkan-drivers=microsoft-experimental): потянет ли Eden v0.2.1 на Dozen — какие расширения и возможности Eden требует (vulkan_device.cpp), что Dozen даёт на D3D12 с Adreno X1-85; известные проблемы;
   - обход в Eden или настройкой, если причина в Eden;
   - zink поверх Dozen — только если два первых не годятся.
D. Обработчик Qualcomm (п.1): есть ли способ лучше, чем cpuopt_fastmem=false (он замедляет CPU), — например, Eden ставит свой обработчик заново после компиляции, обработчик Qualcomm передаёт сигнал дальше по цепочке, или прослойка LD_PRELOAD для sigaction из qcgpuarm64compilercore.so. Докажи по коду dynarmic/Eden (file:line), что выбранный способ безопасен.
E. Дай готовые правки: Dockerfile образа (сейчас ниже), run-eden.sh, настройки Eden (qt-config.ini: Renderer backend, Cpu), патч Eden при необходимости — unified diff. И проверку за один прогон у нас: команды, какие строки в eden_log.txt и gdb должны появиться, какие ожидаются кадры.

ЧТО СЧИТАЕТСЯ ДОКАЗАТЕЛЬСТВОМ
- Символизированный стек с file:line и объяснение, почему цикл бесконечен.
- Для выбранного пути — ссылки на код (Mesa, Eden) и, где можно, сборка и запуск у тебя: например, Mesa d3d12 нельзя запустить без WSL, но Dozen и Eden можно собрать и проверить требования по коду.

ТЕКУЩИЕ ФАЙЛЫ

===== docker/eden-build/Dockerfile (образ сборки, он же база образа стенда) =====
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl git file xz-utils unzip zip \
        build-essential gcc-14 g++-14 cmake ninja-build pkg-config python3 \
        gdb \
        glslang-tools libvulkan-dev spirv-tools \
        libsdl2-dev libssl-dev libusb-1.0-0-dev libudev-dev libasound2-dev libpulse-dev \
        libgl-dev libegl-dev libx11-dev libxext-dev libxrandr-dev \
        libavcodec-dev libavfilter-dev libavutil-dev libswscale-dev \
        zlib1g-dev libzstd-dev liblz4-dev \
        mesa-vulkan-drivers libvulkan1 libgl1 libegl1 libgl1-mesa-dri xvfb \
    && rm -rf /var/lib/apt/lists/*
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        qt6-base-dev qt6-base-private-dev qt6-charts-dev libqt6svg6-dev \
    && rm -rf /var/lib/apt/lists/*
ENV CC=gcc-14 CXX=g++-14
WORKDIR /src

===== docker/eden-patched/Dockerfile (образ стенда) =====
FROM carafe-eden-build
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        xdotool imagemagick x11vnc novnc websockify x11-utils mesa-utils vulkan-tools \
    && rm -rf /var/lib/apt/lists/*
COPY eden /opt/eden/bin/eden
COPY --from=scripts run-eden.sh /usr/local/bin/run-eden
COPY --from=scripts carafe.html /usr/share/novnc/carafe.html
RUN chmod +x /opt/eden/bin/eden /usr/local/bin/run-eden \
    && echo "v0.2.1+carafe aarch64" > /opt/eden/carafe-version
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/run-eden"]

===== docker/eden/run-eden.sh (начало) =====
gpu="${CARAFE_GPU:-wsl}"
case "$gpu" in
    wsl)
        [ -e /dev/dxg ] || exit 64
        export LD_LIBRARY_PATH="/usr/lib/wsl/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
        export GALLIUM_DRIVER=d3d12
        export MESA_D3D12_DEFAULT_ADAPTER_NAME="${CARAFE_ADAPTER:-}"
        export VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/dzn_icd*.json 2>/dev/null | head -n 1)"
        ;;
    software)
        export LIBGL_ALWAYS_SOFTWARE=1
        export MESA_GL_VERSION_OVERRIDE=4.6
        export MESA_GLSL_VERSION_OVERRIDE=460
        export MESA_VK_VERSION_OVERRIDE=1.3
        export VK_ICD_FILENAMES="$(ls /usr/share/vulkan/icd.d/lvp_icd*.json 2>/dev/null | head -n 1)"
        ;;
esac
Xvfb :1 -screen 0 1280x720x24 -nolisten tcp &
export DISPLAY=:1
...
exec /opt/eden/bin/eden -f -g "$1"

===== qt-config.ini, который пишет стенд =====
[UI] check_for_updates=false, confirmStop=2, fullscreen=true, disableControllerApplet=true
[Renderer] backend=1 (Vulkan; в режиме видеокарты 0 = OpenGL_GLSL), resolution_setup=3, fullscreen_mode=0
[Audio] output_engine=3
