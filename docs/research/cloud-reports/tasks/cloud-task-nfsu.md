Ты — инженер, который находит и доказывает, почему Need for Speed Underground (NFSU) с модами сама завершается в эмуляторе Eden, хотя на консоли та же сборка работает. Работай без остановок, пока не докажешь причину железно. Догадка, «вероятно» или «скорее всего» результатом не считаются. Финальный отчёт — на русском, короткими строками, без таблиц, главное в первых строках, не больше ~900 слов.

ЖЁСТКИЕ ОГРАНИЧЕНИЯ
- Ничего не пушить, не создавать PR и ветки на GitHub, не коммитить в удалённые репозитории. Результат — в финальном сообщении, патчи — unified diff прямо в тексте.
- Клонировать для чтения можно:
  - Eden: https://git.eden-emu.dev/eden-emu/eden, v0.2.1, коммит 58c1e20ee58efa3900ba616207d460886214480b; если недоступен, то зеркала на GitHub;
  - Autorun: https://github.com/autorunhq/autorun, коммит 636913ca46a3638018b3ea56995d6852db9f0165 (Wine: dlls/ntdll/unix/horizon.c, virtual.c, signal_*, exception dispatch; FEX: horizon-wine/fex; Box64 WOW64: horizon-wine/source/wow64_box64_*);
  - Atmosphère/Mesosphère: раскладка 32-битного процесса, TLS-страницы потоков, доставка исключений;
  - моды из папки игры: ThirteenAG/WidescreenFixesPack (NFSUnderground.WidescreenFix, сборка с версией файла 15.09.2026), ThirteenAG/Ultimate-ASI-Loader 9.7.4 (dinput8.dll), NFSU_XtendedInput (xan1242), ZPatchU1 (если найдёшь исходники).
- Репозиторий Carafe у тебя есть, это рабочая папка. Важное: docs/autorun.md, раздел «Address space»; docs/research/fixed-base/REPORT.md; runtime/patches/autorun/0006-virtual-32bit-address-space.patch и 0007-low-window-32bit-guest-range.patch; runtime/loader/source/fixed_image.c; app/crates/carafe-core/src/pe.rs и npdm.rs.
- Игру запустить нельзя: нужны ключи консоли и ARM64-хост. Только анализ кода; компилировать куски для проверки можно.

СИСТЕМА
- Carafe собирает NSP для Switch. Внутри — рантайм Autorun: Wine + WOW64 + FEX для 32-битного кода.
- Speed.exe — образ без релокаций с фиксированным адресом 0x400000. Поэтому Carafe собирает NSP с 32-битным адресным пространством (NPDM type 2), с патчами рантайма 0006/0007 и шагом загрузчика fixed_image.c (см. docs).
- Стенд гоняет NSP в Eden v0.2.1: фронтенд Qt, dynarmic, бэкенд arm64, ARM64-хост, Docker/WSL2. В Eden наши патчи 0001–0013, тексты нужных ниже.

ЧТО ИЗВЕСТНО ТОЧНО
1. На консоли NFSU из этой же папки, с этими же модами, работает: широкий экран, геймпад. По docs/research/fixed-base/REPORT.md — 6 запусков из 6, самая долгая сессия 3,8 ч.
2. В Eden оба запуска одинаково завершаются сами через ~29 с, код выхода 0, без падения, до графики. Ни DXVK, ни Vulkan ещё не включались.
3. Папка игры: Speed.exe (3178496 байт), dinput8.dll = Ultimate-ASI-Loader-Win32 9.7.4, scripts/NFSUnderground.WidescreenFix.asi (5552000 байт, версия 15.09.2026, image base 0x10000000), scripts/ZPatchU1.asi, scripts/NFSU_XtendedInput.asi, ini к ним. Папки modloader нет.
   Основные настройки WidescreenFix.ini: FixHUD=1, FixFOV=1, SkipIntro=1, WindowedMode=0, ImproveGamepadSupport=1, ForcedAudioSampleRate=44100, FPSLimit=-1, BlackMagazineFix=1, [CAMERA] Enable=1, ConsoleGamma=1, SMAA=0, HideAABug=2.
4. Журнал рантайма, по порядку (выдержки):
   [BUILD] nx-amd64-fex-2640 … (address space 32 bits)
   [LOWVA] stock layout; fixed-low Win32 titles require the Atmosphere low-address patch
   [CARAFE] alias region 0x0-0x0
   [VA] native stack region 0x200000-0x40000000; window for native mappings 0x18140000-0x40000000; stacks compact
   [VA] early guest reservations: 1404 MB, largest 394 MB; protected from native mappings
   [USD] 0x7ffe0000 unavailable (c0000018); shared user data at 0xffff0000 (00000000)
   [IMAGE] base=0x400000 size=0x395000 … (fixed-address image)
   [INIT] processors=3 memory=2048 MB used=226 MB
   [THREAD] create failed rc=0xd001 threads=15/800 tls_retries=256
   [TLS] 9 placeholder threads (the next refused: rc=0xd001), 2 kept to hold a thread-local page each; of 2 such pages below 4 GB, 0 were inside the program's reserved memory and were taken out of it
   … Wine/WOW64 стартует, грузятся DLL игры …
   [NXLDR] load name=modloader\modloader.asi status=c0000135
   [NXLDR] load name=…\scripts\NFSUnderground.WidescreenFix.asi status=00000000 → attach base=16280000 (перемещён с 0x10000000)
   [NXLDR] load name=xinput1_4.dll status=00000000
   [NXLDR] load name=…\scripts\ZPatchU1.asi status=00000000 → attach base=16160000
   [BOX64EXIT] eip=162e126e esp=0096e24c ebp=0096e260 eax=00000000 ebx=0096e524 ecx=162e126f edx=0096e930 esi=0096e358 edi=0096e930 flags=00000212
   [BOX64EXIT] pc[162e126e]=162e126e NFSUnderground.WidescreenFix.asi+6126e preceding=0c508b4dfc518b550852ff15ccc13016
   [BOX64EXIT] stack 0096e24c: 00eeaed0 00eeaec8 000004b0 00000002
   [BOX64EXIT] stack 0096e25c: 00990040 00000006 00eeaea0 00eeae98
   [BOX64EXIT] stack 0096e26c: 00000480 0000000f 51c31898 00000000
   [BOX64EXIT] stack 0096e29c: 1616e671 1618f600 00ee4130 0096e2b4 (1616e671 = ZPatchU1.asi или exe+e671)
   [LIFECYCLE] final tid=4 code=00000000 … verdict=PASS
   [EXIT] NtTerminateProcess(self) exit_code=0x00000000
5. Байты перед eip: «0c 50 8b 4d fc 51 8b 55 08 52 ff 15 cc c1 30 16», то есть последняя инструкция — call [0x1630c1cc]. В таблице импорта WidescreenFix.asi ячейка RVA 0x8c1cc — KERNEL32!RtlUnwind. Мод раскручивает стек после исключения (SEH), и на этом процесс завершается с кодом 0.
6. Числа на стеке 0x4b0 = 1200 и 0x480 = 1152 похожи на ширину и высоту экрана.

ЗАДАЧА
A. Восстанови точную цепочку: какое исключение возникает, в каком коде (мод, ASI-загрузчик, игра, Wine), почему мод раскручивает стек и почему после этого процесс завершается с кодом 0. Найди это место в исходниках мода нужной версии (и ZPatchU1, если он участвует: на стеке его адрес).
B. Найди, чем Eden отличается от консоли так, что на консоли этого исключения нет. Проверь все кандидаты и закрой каждый доказательством «да» или «нет»:
   - раскладка 32-битного процесса в Eden против Mesosphère: регионы, TLS-страницы потоков. Строки [THREAD] create failed rc=0xd001 на 15-м потоке и [TLS] … ниже 4 GB: это норма для 32-битного процесса или ошибка Eden? Связано ли с завершением?
   - доставка исключений и data abort процессу: наши патчи Eden 0005, 0009, 0010 против того, как это делает Mesosphère (кадр исключения, регистры, повторный вход, svcReturnFromException);
   - права страниц, VirtualProtect/VirtualQuery, PAGE_GUARD, запись в код игры (моды патчат Speed.exe в памяти);
   - всё, что мод запрашивает у системы на старте: экран (1200x1152?), XInput, звук, время, файлы и ini.
C. Отдельно докажи, что причина не в Carafe, не в рантайме Autorun и не в Wine: на консоли тот же код работает. Если причина в наших патчах Eden 0005/0009/0010, так и скажи и покажи, где они расходятся с Mesosphère.
D. Дай минимальный патч Eden (unified diff против v0.2.1 с нашими патчами) и способ проверить его за один прогон: какая строка журнала изменится. Если для доказательства нужен лог в Eden или в рантайме, дай патч с логом: Eden у нас пересобирается за минуту, рантайм — за 4.

ЧТО СЧИТАЕТСЯ ДОКАЗАТЕЛЬСТВОМ
- Цепочка с точными ссылками file:line: мод → Wine/Autorun → Eden, и место расхождения Eden с Mesosphère.
- Объяснение каждого наблюдения из пунктов 2–6 этой цепочкой.
- Явный ответ, почему на консоли этого не происходит.

ТЕКСТЫ НАШИХ ПАТЧЕЙ EDEN
Порядок наложения 0001…0013. Кратко:
- 0001: пропуск методов на непривязанном subchannel;
- 0002: nvnflinger отпускает последний кадр отключённого слоя;
- 0003: BindObject 3D в subchannel 0;
- 0004: FPCR trap enables сняты (хост Snapdragon);
- 0006: ic ivau/iallu на всех ядрах;
- 0007: svcInvalidate/StoreProcessDataCache;
- 0011: FreeSpace после UnmapBuffer;
- 0012: выгрузка изменённой GPU памяти при каждом чтении процессором;
- 0013: размер шейдера без self-branch считается после заголовка;
- 0008: код 39-битного процесса на 64 ГиБ вместо 2 ГиБ (code_address = 0x10'0000'0000 + aslr_space_offset в KProcess::LoadFromMetadata), к 32-битному процессу NFSU не относится.
Полные тексты 0005, 0009, 0010 ниже.

===== 0005-kernel-deliver-user-exceptions-and-return-from-them.patch =====
Carafe bench: user-mode exceptions go to the process's handler and svcReturnFromException returns from it,
as in Mesosphère: the frame goes to the process local region at 0x148, x0 holds the type, x1 the frame,
pc the process entry point. The runtime enters FEX through a brk that only its handler resolves.
The handler gets the exception before Eden's debugger, so the gdb stub can watch a process that runs FEX.
When no handler takes it, Eden behaves as before.
GPL-3.0-or-later, as the files it changes.

--- a/src/core/arm/dynarmic/arm_dynarmic_64.cpp
+++ b/src/core/arm/dynarmic/arm_dynarmic_64.cpp
@@ -135,12 +135,10 @@
         ReturnException(pc, PrefetchAbort);
         return;
     default:
-        if (m_debugger_enabled) {
-            ReturnException(pc, InstructionBreakpoint);
-        } else {
-            m_parent.LogBacktrace(m_process);
-            LOG_CRITICAL(Core_ARM, "ExceptionRaised(exception = {}, pc = {:08X}, code = {:08X})", static_cast<std::size_t>(exception), pc, m_memory.Read32(pc));
+        if (!m_debugger_enabled) {
+            LOG_DEBUG(Core_ARM, "ExceptionRaised(exception = {}, pc = {:08X}, code = {:08X})", static_cast<std::size_t>(exception), pc, m_memory.Read32(pc));
         }
+        ReturnException(pc, InstructionBreakpoint);
     }
 }
 
--- a/src/core/hle/kernel/physical_core.cpp
+++ b/src/core/hle/kernel/physical_core.cpp
@@ -4,6 +4,7 @@
 // SPDX-FileCopyrightText: Copyright 2020 yuzu Emulator Project
 // SPDX-License-Identifier: GPL-2.0-or-later
 
+#include "common/alignment.h"
 #include "common/scope_exit.h"
 #include "common/settings.h"
 #include "core/core.h"
@@ -16,6 +17,59 @@
 
 namespace Kernel {
 
+namespace {
+
+bool EnterUserExceptionHandler(KernelCore& kernel, KProcess& process, Core::ArmInterface& interface,
+                               bool prefetch_abort) {
+    if (!process.Is64Bit()) {
+        return false;
+    }
+
+    Svc::ThreadContext context{};
+    interface.GetContext(context);
+
+    u32 esr = 1U << 25;
+    u64 far = 0;
+    if (prefetch_abort) {
+        esr |= 0x20U << 26;
+        far = context.pc;
+    } else if (const u32 insn = process.GetMemory().Read32(context.pc); (insn & 0xFFE0001F) == 0xD4200000) {
+        esr |= (0x3CU << 26) | ((insn >> 5) & 0xFFFF);
+    }
+
+    if (!process.EnterUserException()) {
+        return false;
+    }
+
+    Svc::ExceptionInfo64 info{};
+    for (size_t i = 0; i < info.r.size(); i++) {
+        info.r[i] = context.r[i];
+    }
+    info.lr = context.lr;
+    info.sp = context.sp;
+    info.pc = context.pc;
+    info.pstate = context.pstate & Svc::ExceptionInfoPstateMask;
+    info.esr = esr;
+    info.far = far;
+
+    const u64 info_address =
+        GetInteger(process.GetProcessLocalRegionAddress()) + Svc::ProcessLocalRegionExceptionInfoOffset;
+    process.GetMemory().WriteBlock(info_address, &info, sizeof(info));
+
+    context.pc = GetInteger(process.GetEntryPoint());
+    context.r[0] = Svc::ExceptionTypeInstructionAbort;
+    context.r[1] = info_address;
+    context.sp = Common::AlignDown(info_address, 0x10);
+    context.pstate = 0;
+
+    auto* thread = GetCurrentThreadPointer(kernel);
+    thread->GetContext() = context;
+    kernel.CurrentPhysicalCore().LoadContext(thread);
+    return true;
+}
+
+}
+
 PhysicalCore::PhysicalCore(KernelCore& kernel, std::size_t core_index)
     : m_kernel{kernel}, m_core_index{core_index} {
     m_is_single_core = !kernel.IsMulticore();
@@ -130,10 +184,20 @@ void PhysicalCore::RunThread(Kernel::KThread* thread) {
                 // state.
                 interface->GetContext(thread->GetContext());
             }
+            if (EnterUserExceptionHandler(m_kernel, *process, *interface, prefetch_abort)) {
+                return;
+            }
             if (system.DebuggerEnabled()) {
                 system.GetDebugger().NotifyThreadStopped(thread);
             } else {
                 interface->LogBacktrace(process);
+                if (breakpoint) {
+                    Svc::ThreadContext context{};
+                    interface->GetContext(context);
+                    context.pc += 4;
+                    interface->SetContext(context);
+                    continue;
+                }
             }
             thread->RequestSuspend(SuspendType::Debug);
             return;
--- a/src/core/hle/kernel/svc.cpp
+++ b/src/core/hle/kernel/svc.cpp
@@ -1532,6 +1532,7 @@
 static void SvcWrap_ReturnFromException64(Core::System& system, std::span<uint64_t, 8> args) {
     Result result = Convert<Result>(GetArg64(args, 0));
     ReturnFromException64(system, result);
+    system.Kernel().CurrentPhysicalCore().SaveSvcArguments(GetCurrentProcess(system.Kernel()), args);
 }
 
 static void SvcWrap_GetInfo64(Core::System& system, std::span<uint64_t, 8> args) {
--- a/src/core/hle/kernel/svc/svc_exception.cpp
+++ b/src/core/hle/kernel/svc/svc_exception.cpp
@@ -117,7 +117,40 @@
 }
 
 void ReturnFromException(Core::System& system, Result result) {
-    UNIMPLEMENTED();
+    auto& kernel = system.Kernel();
+    auto& process = GetCurrentProcess(kernel);
+    if (!process.Is64Bit()) {
+        UNIMPLEMENTED();
+        return;
+    }
+
+    const u64 info_address =
+        GetInteger(process.GetProcessLocalRegionAddress()) + ProcessLocalRegionExceptionInfoOffset;
+    ExceptionInfo64 info{};
+    process.GetMemory().ReadBlock(info_address, &info, sizeof(info));
+
+    if (!process.LeaveUserException()) {
+        return;
+    }
+
+    auto* thread = GetCurrentThreadPointer(kernel);
+    auto& core = kernel.CurrentPhysicalCore();
+    core.SaveContext(thread);
+    auto& context = thread->GetContext();
+    for (size_t i = 0; i < info.r.size(); i++) {
+        context.r[i] = info.r[i];
+    }
+    context.lr = info.lr;
+    context.sp = info.sp;
+    context.pc = info.pc;
+    context.pstate = (info.pstate & ExceptionInfoPstateMask) | (context.pstate & ~ExceptionInfoPstateMask);
+    core.LoadContext(thread);
+
+    if (R_FAILED(result)) {
+        LOG_CRITICAL(Kernel_SVC, "Unhandled user exception at pc={:#x}, result={:#x}", info.pc, result.raw);
+        core.LogBacktrace();
+        thread->RequestSuspend(SuspendType::Debug);
+    }
 }
 
 void Break64(Core::System& system, BreakReason break_reason, uint64_t arg, uint64_t size) {
--- a/src/core/hle/kernel/svc_types.h
+++ b/src/core/hle/kernel/svc_types.h
@@ -122,6 +122,23 @@
 
 constexpr inline size_t ThreadLocalRegionSize = 0x200;
 
+constexpr inline size_t ProcessLocalRegionExceptionInfoOffset = 0x148;
+constexpr inline u64 ExceptionTypeInstructionAbort = 0x100;
+constexpr inline u32 ExceptionInfoPstateMask = 0xF0000000;
+
+struct ExceptionInfo64 {
+    std::array<u64, 9> r;
+    u64 lr;
+    u64 sp;
+    u64 pc;
+    u32 pstate;
+    u32 afsr0;
+    u32 afsr1;
+    u32 esr;
+    u64 far;
+};
+static_assert(sizeof(ExceptionInfo64) == 0x78);
+
 struct PageInfo {
     u32 flags;
 };

===== 0009-kernel-deliver-data-aborts-to-the-process.patch =====
Carafe bench: an access to unmapped memory stops the JIT at that instruction and goes to the process's
exception handler as a data abort with its address, as on the hardware, instead of reading zero and
dropping the write. The runtime commits reserved memory lazily from that handler.
When no handler takes it, the thread stops.
GPL-3.0-or-later, as the files it changes.

--- a/src/core/arm/arm_interface.h
+++ b/src/core/arm/arm_interface.h
@@ -4,8 +4,10 @@
 #pragma once
 
 #include <array>
+#include <optional>
 #include <span>
 #include <string>
+#include <utility>
 #include <vector>
 
 #include "common/common_funcs.h"
@@ -39,6 +41,11 @@ enum class HaltReason : u64 {
 };
 DECLARE_ENUM_FLAG_OPERATORS(HaltReason);
 
+struct MemoryAbort {
+    u64 address;
+    bool write;
+};
+
 enum class Architecture {
     AArch64,
     AArch32,
@@ -101,12 +108,17 @@ public:
     virtual const Kernel::DebugWatchpoint* HaltedWatchpoint() const = 0;
     virtual void RewindBreakpointInstruction() = 0;
 
+    std::optional<MemoryAbort> TakeMemoryAbort() {
+        return std::exchange(m_memory_abort, std::nullopt);
+    }
+
 protected:
     const Kernel::DebugWatchpoint* MatchingWatchpoint(
         u64 addr, u64 size, Kernel::DebugWatchpointType access_type) const;
 
 protected:
     const WatchpointArray* m_watchpoints{};
+    std::optional<MemoryAbort> m_memory_abort{};
     bool m_uses_wall_clock{};
 };
 
--- a/src/core/arm/dynarmic/arm_dynarmic_64.cpp
+++ b/src/core/arm/dynarmic/arm_dynarmic_64.cpp
@@ -18,7 +18,7 @@ using namespace Common::Literals;
 DynarmicCallbacks64::DynarmicCallbacks64(ArmDynarmic64& parent, Kernel::KProcess* process)
     : m_parent{parent}, m_memory(process->GetMemory())
     , m_process(process), m_debugger_enabled{parent.m_system.DebuggerEnabled()}
-    , m_check_memory_access{m_debugger_enabled || !Settings::values.cpuopt_ignore_memory_aborts.GetValue()}
+    , m_check_memory_access{true}
 {}
 
 u8 DynarmicCallbacks64::MemoryRead8(u64 vaddr) {
@@ -185,9 +185,10 @@ bool DynarmicCallbacks64::CheckMemoryAccess(u64 addr, u64 size, Kernel::DebugWat
     }
 
     if (!m_memory.IsValidVirtualAddressRange(addr, size)) {
-        LOG_CRITICAL(Core_ARM, "Stopping execution due to unmapped memory access at {:#x}",
-                        addr);
-        m_parent.m_jit->HaltExecution(PrefetchAbort);
+        if (!m_parent.m_memory_abort) {
+            m_parent.m_memory_abort = MemoryAbort{addr, type == Kernel::DebugWatchpointType::Write};
+        }
+        m_parent.m_jit->HaltExecution(DataAbort);
         return false;
     }
 
@@ -267,9 +268,7 @@ void ArmDynarmic64::MakeJit(Common::PageTable* page_table, std::size_t address_s
 #endif
 
     // Allow memory fault handling to work
-    if (m_system.DebuggerEnabled()) {
-        config.check_halt_on_memory_access = true;
-    }
+    config.check_halt_on_memory_access = true;
 
     // null_jit
     if (!page_table) {
--- a/src/core/hle/kernel/physical_core.cpp
+++ b/src/core/hle/kernel/physical_core.cpp
@@ -20,7 +20,8 @@ namespace Kernel {
 namespace {
 
 bool EnterUserExceptionHandler(KernelCore& kernel, KProcess& process, Core::ArmInterface& interface,
-                               bool prefetch_abort) {
+                               bool prefetch_abort,
+                               const std::optional<Core::MemoryAbort>& memory_abort = std::nullopt) {
     if (!process.Is64Bit()) {
         return false;
     }
@@ -30,7 +31,12 @@ bool EnterUserExceptionHandler(KernelCore& kernel, KProcess& process, Core::ArmI
 
     u32 esr = 1U << 25;
     u64 far = 0;
-    if (prefetch_abort) {
+    u64 type = Svc::ExceptionTypeInstructionAbort;
+    if (memory_abort) {
+        esr |= (0x24U << 26) | (memory_abort->write ? 1U << 6 : 0U) | 0x07U;
+        far = memory_abort->address;
+        type = Svc::ExceptionTypeDataAbort;
+    } else if (prefetch_abort) {
         esr |= 0x20U << 26;
         far = context.pc;
     } else if (const u32 insn = process.GetMemory().Read32(context.pc); (insn & 0xFFE0001F) == 0xD4200000) {
@@ -57,7 +63,7 @@ bool EnterUserExceptionHandler(KernelCore& kernel, KProcess& process, Core::ArmI
     process.GetMemory().WriteBlock(info_address, &info, sizeof(info));
 
     context.pc = GetInteger(process.GetEntryPoint());
-    context.r[0] = Svc::ExceptionTypeInstructionAbort;
+    context.r[0] = type;
     context.r[1] = info_address;
     context.sp = Common::AlignDown(info_address, 0x10);
     context.pstate = 0;
@@ -173,6 +179,19 @@ void PhysicalCore::RunThread(Kernel::KThread* thread) {
         // Since scheduling may occur here, we cannot use any cached
         // state after returning from calls we make.
 
+        if (data_abort) {
+            if (const auto memory_abort = interface->TakeMemoryAbort()) {
+                if (EnterUserExceptionHandler(m_kernel, *process, *interface, false, memory_abort)) {
+                    return;
+                }
+                LOG_CRITICAL(Core_ARM, "Unhandled {} of unmapped memory at {:#x}",
+                             memory_abort->write ? "write" : "read", memory_abort->address);
+                interface->LogBacktrace(process);
+                thread->RequestSuspend(SuspendType::Debug);
+                return;
+            }
+        }
+
         // Notify the debugger and go to sleep if a breakpoint was hit,
         // or if the thread is unable to continue for any reason.
         if (breakpoint || prefetch_abort) {
--- a/src/core/hle/kernel/svc_types.h
+++ b/src/core/hle/kernel/svc_types.h
@@ -124,6 +124,7 @@ constexpr inline size_t ThreadLocalRegionSize = 0x200;
 
 constexpr inline size_t ProcessLocalRegionExceptionInfoOffset = 0x148;
 constexpr inline u64 ExceptionTypeInstructionAbort = 0x100;
+constexpr inline u64 ExceptionTypeDataAbort = 0x101;
 constexpr inline u32 ExceptionInfoPstateMask = 0xF0000000;
 
 struct ExceptionInfo64 {

===== 0010-memory-permission-faults-on-guarded-pages.patch =====
Carafe bench: a mapped page the guest may not access (permission None or read-only) gives a permission fault
(DFSC 0x0F) with its address, as on Horizon. Eden changed only the host mapping for fastmem, so a write to a
guard page went through: FEX's call-ret stack ran through its guard and wrote below its region.
Each page keeps its guest permissions; a page that is not read-write takes the memory callbacks, which check them.
Based on an outside analysis of the crash (eden-0010-guest-page-permissions).
GPL-3.0-or-later, as the files it changes.

--- a/src/common/page_table.h
+++ b/src/common/page_table.h
@@ -133,11 +133,15 @@
 
     /// Vector of memory pointers backing each page. An entry can only be non-null if the
     /// corresponding attribute element is of type `Memory`.
+    static constexpr u64 PermRead = 1;
+    static constexpr u64 PermWrite = 2;
+    static constexpr u64 PermSlowPath = 4;
+
     struct PageEntryData {
         PageInfo ptr;
         u64 block;
         u64 addr;
-        u64 padding;
+        u64 perms;
     };
     VirtualBuffer<PageEntryData> entries;
     static_assert(sizeof(PageEntryData) == 32);
--- a/src/core/memory.h
+++ b/src/core/memory.h
@@ -137,6 +137,9 @@
      */
     [[nodiscard]] bool IsValidVirtualAddressRange(Common::ProcessAddress base, u64 size) const;
 
+    [[nodiscard]] std::optional<u64> FirstDeniedAddress(Common::ProcessAddress base, u64 size,
+                                                        bool write) const;
+
     /**
      * Gets a pointer to the given address.
      *
--- a/src/core/memory.cpp
+++ b/src/core/memory.cpp
@@ -8,6 +8,7 @@
 #include <algorithm>
 #include <cstring>
 #include <mutex>
+#include <optional>
 #include <span>
 #include <thread>
 #include <type_traits>
@@ -73,12 +74,65 @@
                    GetInteger(target));
         MapPages(page_table, base / YUZU_PAGESIZE, size / YUZU_PAGESIZE, target,
                  Common::PageType::Memory);
+        SetPagePermissions(page_table, GetInteger(base), size, perms);
 
         if (current_page_table->fastmem_arena) {
             host_buffer->Map(GetInteger(base), GetInteger(target) - DramMemoryMap::Base, size, perms, separate_heap);
         }
     }
 
+    void SetPagePermissions(Common::PageTable& page_table, u64 vaddr, u64 size,
+                            Common::MemoryPermission perms) {
+        u64 bits = 0;
+        if (True(perms & Common::MemoryPermission::Read)) {
+            bits |= Common::PageTable::PermRead;
+        }
+        if (True(perms & Common::MemoryPermission::Write)) {
+            bits |= Common::PageTable::PermWrite;
+        }
+        const bool restricted =
+            bits != (Common::PageTable::PermRead | Common::PageTable::PermWrite);
+
+        for (u64 addr = vaddr; addr < vaddr + size; addr += YUZU_PAGESIZE) {
+            auto& entry = page_table.entries[addr >> YUZU_PAGEBITS];
+            const bool forced = (entry.perms & Common::PageTable::PermSlowPath) != 0;
+            u64 new_bits = bits;
+            switch (entry.ptr.Type()) {
+            case Common::PageType::Memory:
+                if (restricted) {
+                    entry.ptr.Store(0, Common::PageType::DebugMemory);
+                    new_bits |= Common::PageTable::PermSlowPath;
+                }
+                break;
+            case Common::PageType::DebugMemory:
+                if (forced && restricted) {
+                    new_bits |= Common::PageTable::PermSlowPath;
+                } else if (forced) {
+                    u8* const pointer = system.DeviceMemory().GetPointer<u8>(entry.addr + addr);
+                    entry.ptr.Store(uintptr_t(pointer) - addr, Common::PageType::Memory);
+                }
+                break;
+            default:
+                break;
+            }
+            entry.perms = new_bits;
+        }
+    }
+
+    std::optional<u64> FirstDeniedAddress(u64 vaddr, u64 size, bool write) const {
+        const u64 needed = write ? Common::PageTable::PermWrite : Common::PageTable::PermRead;
+        const u64 end = vaddr + size;
+        for (u64 page = vaddr & ~YUZU_PAGEMASK; page < end; page += YUZU_PAGESIZE) {
+            if (!AddressSpaceContains(*current_page_table, page, 1)) {
+                return (std::max)(page, vaddr);
+            }
+            if ((current_page_table->entries[page >> YUZU_PAGEBITS].perms & needed) == 0) {
+                return (std::max)(page, vaddr);
+            }
+        }
+        return std::nullopt;
+    }
+
     void UnmapRegion(Common::PageTable& page_table, Common::ProcessAddress base, u64 size,
                      bool separate_heap) {
         ASSERT_MSG((size & YUZU_PAGEMASK) == 0, "non-page aligned size: {:016X}", size);
@@ -96,6 +150,8 @@
         ASSERT_MSG((size & YUZU_PAGEMASK) == 0, "non-page aligned size: {:016X}", size);
         ASSERT_MSG((vaddr & YUZU_PAGEMASK) == 0, "non-page aligned base: {:016X}", vaddr);
 
+        SetPagePermissions(*current_page_table, vaddr, size, perms);
+
         if (!current_page_table->fastmem_arena) {
             return;
         }
@@ -433,6 +489,10 @@
                     // Don't mess with already non-debug or rasterizer memory.
                     break;
                 case Common::PageType::DebugMemory: {
+                    if (current_page_table->entries[vaddr >> YUZU_PAGEBITS].perms &
+                        Common::PageTable::PermSlowPath) {
+                        break;
+                    }
                     u8* const pointer = GetPointerFromDebugMemory(vaddr & ~YUZU_PAGEMASK);
                     current_page_table->entries[vaddr >> YUZU_PAGEBITS].ptr.Store(uintptr_t(pointer) - (vaddr & ~YUZU_PAGEMASK), Common::PageType::Memory);
                     break;
@@ -478,6 +538,7 @@
                 case Common::PageType::DebugMemory:
                 case Common::PageType::Memory:
                     current_page_table->entries[vaddr >> YUZU_PAGEBITS].ptr.Store(0, Common::PageType::RasterizerCachedMemory);
+                    current_page_table->entries[vaddr >> YUZU_PAGEBITS].perms &= ~Common::PageTable::PermSlowPath;
                     break;
                 case Common::PageType::RasterizerCachedMemory:
                     // There can be more than one GPU region mapped per CPU region, so it's common
@@ -504,6 +565,11 @@
                         // pagetable after unmapping a VMA. In that case the underlying VMA will no
                         // longer exist, and we should just leave the pagetable entry blank.
                         current_page_table->entries[vaddr >> YUZU_PAGEBITS].ptr.Store(0, Common::PageType::Unmapped);
+                    } else if (auto& entry = current_page_table->entries[vaddr >> YUZU_PAGEBITS];
+                               (entry.perms & (Common::PageTable::PermRead | Common::PageTable::PermWrite)) !=
+                               (Common::PageTable::PermRead | Common::PageTable::PermWrite)) {
+                        entry.ptr.Store(0, Common::PageType::DebugMemory);
+                        entry.perms |= Common::PageTable::PermSlowPath;
                     } else {
                         current_page_table->entries[vaddr >> YUZU_PAGEBITS].ptr.Store(uintptr_t(pointer) - (vaddr & ~YUZU_PAGEMASK), Common::PageType::Memory);
                     }
@@ -543,6 +609,7 @@
                 page_table.entries[base].ptr.Store(0, type);
                 page_table.entries[base].addr = 0;
                 page_table.entries[base].block = 0;
+                page_table.entries[base].perms = 0;
                 base += 1;
             }
         } else {
@@ -553,6 +620,8 @@
                 page_table.entries[base].ptr.Store(host_ptr, type);
                 page_table.entries[base].addr = backing;
                 page_table.entries[base].block = orig_base << YUZU_PAGEBITS;
+                page_table.entries[base].perms =
+                    Common::PageTable::PermRead | Common::PageTable::PermWrite;
 
                 ASSERT_MSG(page_table.entries[base].ptr.Pointer(),
                            "memory mapping base yield a nullptr within the table");
@@ -805,6 +874,11 @@
     impl->ProtectRegion(page_table, GetInteger(vaddr), size, perms);
 }
 
+std::optional<u64> Memory::FirstDeniedAddress(Common::ProcessAddress vaddr, u64 size,
+                                              bool write) const {
+    return impl->FirstDeniedAddress(GetInteger(vaddr), size, write);
+}
+
 bool Memory::IsValidVirtualAddress(const Common::ProcessAddress vaddr) const {
     const auto& page_table = *impl->current_page_table;
     const size_t page = vaddr >> YUZU_PAGEBITS;
--- a/src/core/arm/arm_interface.h
+++ b/src/core/arm/arm_interface.h
@@ -44,6 +44,7 @@
 struct MemoryAbort {
     u64 address;
     bool write;
+    bool permission{};
 };
 
 enum class Architecture {
--- a/src/core/arm/dynarmic/arm_dynarmic_64.cpp
+++ b/src/core/arm/dynarmic/arm_dynarmic_64.cpp
@@ -192,6 +192,15 @@
         return false;
     }
 
+    const bool write = type == Kernel::DebugWatchpointType::Write;
+    if (const auto denied = m_memory.FirstDeniedAddress(addr, size, write)) {
+        if (!m_parent.m_memory_abort) {
+            m_parent.m_memory_abort = MemoryAbort{*denied, write, true};
+        }
+        m_parent.m_jit->HaltExecution(DataAbort);
+        return false;
+    }
+
     if (!m_debugger_enabled) {
         return true;
     }
--- a/src/core/hle/kernel/physical_core.cpp
+++ b/src/core/hle/kernel/physical_core.cpp
@@ -33,7 +33,8 @@
     u64 far = 0;
     u64 type = Svc::ExceptionTypeInstructionAbort;
     if (memory_abort) {
-        esr |= (0x24U << 26) | (memory_abort->write ? 1U << 6 : 0U) | 0x07U;
+        const u32 fault_status = memory_abort->permission ? 0x0FU : 0x07U;
+        esr |= (0x24U << 26) | (memory_abort->write ? 1U << 6 : 0U) | fault_status;
         far = memory_abort->address;
         type = Svc::ExceptionTypeDataAbort;
     } else if (prefetch_abort) {
