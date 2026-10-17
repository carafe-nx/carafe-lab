Ты — инженер, который доводит до конца разбор падения в эмуляторе Eden. Работай без остановок, пока не получишь обоснованный ответ. Финальный отчёт — на русском, короткими строками, без таблиц.

ЖЁСТКИЕ ОГРАНИЧЕНИЯ
- Ничего не пушить, не создавать PR, не коммитить ни в какие удалённые репозитории, не создавать веток на GitHub. Работай только локально в своей облачной машине; результаты — в финальном сообщении.
- Можно клонировать публичные исходники для чтения: Eden (https://git.eden-emu.dev/eden-emu/eden, тег v0.2.1, коммит 58c1e20ee58efa3900ba616207d460886214480b; при недоступности — зеркала на GitHub), Autorun (https://github.com/autorunhq/autorun, коммит 636913ca46a3638018b3ea56995d6852db9f0165), FEX-Emu (https://github.com/FEX-Emu/FEX), Atmosphère/Mesosphère (https://github.com/Atmosphere-NX/Atmosphere), libnx (https://github.com/switchbrew/libnx), Wine.
- Запустить игру нельзя (нужны ключи консоли и ARM64-хост) — только анализ кода, при желании компиляция кусков для проверки патчей.

СИСТЕМА
- Carafe собирает NSP для Switch с кастомной прошивкой: внутри рантайм Autorun = Wine (dlls/ntdll/unix/horizon.c, встроенный в процесс сервер Wine) + WOW64 + FEX (libwow64fex, патч Autorun horizon-wine/fex/horizon.patch). FEX входит через `brk #0xfec0`, который разрешает только пользовательский обработчик исключений процесса (libnx __libnx_exception_handler в horizon.c), возврат — svcReturnFromException.
- Wine в Autorun резервирует память и мапит её лениво: первое касание даёт data abort, обработчик `horizon_commit_lazy_fault` (horizon.c ~19593) мапит кусок через svcMapProcessCodeMemory и повторяет инструкцию. Освобождение — svcUnmapProcessCodeMemory (`unmap_code_memory_range`, horizon.c ~17489).
- Тестовый стенд гоняет NSP в Eden v0.2.1 (Qt-фронтенд, dynarmic, бэкенд arm64 на ARM64-хосте Snapdragon, WSL2/Docker, рендер — программный Vulkan). Игра — OpenTTD 15.3 win32. На настоящей консоли та же сборка работает.

НАШИ ПАТЧИ EDEN (применяются по порядку)
- 0001 dma_pusher: методы на непривязанном subchannel пропускаются.
- 0002 nvnflinger: отпускать последний кадр отключённого слоя.
- 0003 nvhost_gpu: при выделении 3D-контекста BindObject 0xB197 в subchannel 0.
- 0004 dynarmic arm64: биты FPCR trap enables (0x9F00) всегда сняты, как на Cortex-A57.
- 0005 пользовательские исключения. arm_dynarmic_64.cpp ExceptionRaised(default) → ReturnException(pc, InstructionBreakpoint) всегда. physical_core.cpp:
```
namespace {
bool EnterUserExceptionHandler(KernelCore& kernel, KProcess& process, Core::ArmInterface& interface,
                               bool prefetch_abort,
                               const std::optional<Core::MemoryAbort>& memory_abort = std::nullopt) {
    if (!process.Is64Bit()) return false;
    Svc::ThreadContext context{};
    interface.GetContext(context);
    u32 esr = 1U << 25; u64 far = 0; u64 type = Svc::ExceptionTypeInstructionAbort;
    if (memory_abort) { esr |= (0x24U << 26) | (memory_abort->write ? 1U << 6 : 0U) | 0x07U; far = memory_abort->address; type = Svc::ExceptionTypeDataAbort; }
    else if (prefetch_abort) { esr |= 0x20U << 26; far = context.pc; }
    else if (const u32 insn = process.GetMemory().Read32(context.pc); (insn & 0xFFE0001F) == 0xD4200000) { esr |= (0x3CU << 26) | ((insn >> 5) & 0xFFFF); }
    if (!process.EnterUserException()) return false;          // может ждать: другой поток уже в обработчике
    Svc::ExceptionInfo64 info{}; /* r[0..8], lr, sp, pc, pstate&0xF0000000, esr, far из context */
    const u64 info_address = GetInteger(process.GetProcessLocalRegionAddress()) + 0x148;
    process.GetMemory().WriteBlock(info_address, &info, sizeof(info));
    context.pc = GetInteger(process.GetEntryPoint()); context.r[0] = type; context.r[1] = info_address;
    context.sp = Common::AlignDown(info_address, 0x10); context.pstate = 0;
    auto* thread = GetCurrentThreadPointer(kernel);
    thread->GetContext() = context;
    kernel.CurrentPhysicalCore().LoadContext(thread);
    return true;
}
}
// в RunThread: data_abort → TakeMemoryAbort → EnterUserExceptionHandler → return;
// breakpoint||prefetch_abort: RewindBreakpointInstruction; GetContext(thread->GetContext());
//   if (EnterUserExceptionHandler(...)) return;  else (без отладчика) LogBacktrace; brk → pc+=4; continue;
```
  svc_exception.cpp ReturnFromException: читает ExceptionInfo64 из PLR+0x148; `if (!process.LeaveUserException()) return;` затем `core = kernel.CurrentPhysicalCore(); core.SaveContext(thread); context.r[0..8], lr, sp, pc, pstate(маска) = info; core.LoadContext(thread);` при R_FAILED(result) — лог и RequestSuspend. В svc.cpp SvcWrap_ReturnFromException64 после вызова `CurrentPhysicalCore().SaveSvcArguments(process, args)`, чтобы диспетчер не затёр восстановленные x0..x7. Раньше после ожидания в EnterUserException мы писали регистры в `interface` старого ядра и делали `continue` — исправлено на LoadContext в текущее ядро + return; падение осталось.
- 0006 ic ivau / ic iallu: InvalidateCacheRange/ClearInstructionCache на JIT всех ядер процесса (`m_process->GetArmInterface(i)` по NUM_CPU_CORES) + сброс у каждого ядра копии последней страницы кода (`last_code_addr` стал std::atomic<u64>, в MemoryReadCode адрес пишется до ReadBlock).
- 0007 svcInvalidateProcessDataCache / svcStoreProcessDataCache реализованы как Flush.
- 0008 k_process.cpp: для 39-битного процесса без ASLR `code_address = 0x10'0000'0000 + aslr_space_offset` (вместо 2 ГиБ).
- 0009 доставка data abort: m_check_memory_access = true всегда; config.check_halt_on_memory_access = true всегда; в CheckMemoryAccess для невалидного адреса `m_parent.m_memory_abort = MemoryAbort{addr, type==Write}` (если ещё пусто) и `HaltExecution(DataAbort)` (бит MemoryAbort dynarmic — точная остановка на инструкции, PC = начало инструкции); ArmInterface получил `std::optional<MemoryAbort> m_memory_abort` и `TakeMemoryAbort()`.

ЧТО УЖЕ ДОКАЗАНО
1. Без 0009 Eden глушил обращения к незамапленной памяти (чтение 0, запись теряется), ленивая память Wine не появлялась, игра «живо» висла на ~25 с. С 0009 игра идёт дальше; из ~120 сбоев за запуск Wine ~119 штатно домапливает.
2. Падение без аппаратного ускорения (второй запуск, ~45–60 с после старта) — 0xc0000005, недетерминированно (в 1 из 6 прогонов дошли до меню — в медленном verbose-режиме). Механизм (журнал SVC с номерами потоков Eden):
```
 7.888 MAP-CODE 3ffca90000 size=402000  t=76   (главный поток, старт FEX)
 7.905 MAP-CODE 3ffc290000 size=800000  t=76
 7.922 MAP-CODE 3ffb290000 size=1000000 t=76
 ... каждый поток FEX на старте мапит себе ровно такие три области (4МБ+8К, 8МБ, 16МБ)
36.99 MAP-CODE 3fa8400000 size=402000  t=235
37.16 MAP-CODE 3fa7c00000 size=800000  t=235
37.18 MAP-CODE 3fa6c00000 size=1000000 t=235
45.37 UNMAP    3fed7c1000 size=400000  t=205   (поток 205 освобождает СВОЮ 4МБ — норма)
51.50–51.51 ExitThread t=241, 264, 265
51.5607 UNMAP 3ffc290000 size=800000  t=235   ← поток 235 освобождает 8МБ ГЛАВНОГО потока
51.5623 UNMAP 3ffb290000 size=1000000 t=235   ← и 16МБ главного
51.7675 memory abort: read 0x3ffc8c5f60 pc=JIT(FEX) thread=76 → не домаплено → AV → игра падает
```
  Регистры: у 235 x18(TEB)=0xB79C000, у 76 x18=0x8FD8000 — не перепутаны. Стек 235 в момент UNMAP: svcUnmapProcessCodeMemory ← unmap_code_memory_range ← unmap_range_locked ← horizon_munmap ← delete_view (virtual.c) ← free_pages ← NtFreeVirtualMemory ← wine_nx_do_syscall ← __wine_syscall_dispatcher ← (кадры JIT FEX не раскручены) ← signal_start_thread ← server_init_thread ← __thread_entry. В другом прогоне было зеркально: поток 234 снял область главного 76, а 76 снял область 234 (с интервалом 10 мс).
  FEX в Autorun, BTCpuThreadTerm(HANDLE Thread, LONG ExitCode): `Self = ThreadTID == GetCurrentThreadId()`; если !Self — RtlWow64SuspendThread(ThreadDup); `GetThreadTLS(ThreadDup)`; `Threads.find(ThreadTID)`; затем CallRetStack::DestroyThread, CTX->DestroyThread(ThreadState), `TLS.ThreadState() = nullptr`; Self → DeinitCRTThread. ThreadTID — через NtQueryInformationThread(ThreadBasicInformation) → встроенный сервер Wine: horizon.c `horizon_server_handle_get_thread_info` (~11833) → `horizon_server_get_thread_locked(request->handle, &status)` → reply.tid/teb.
3. Служебный поток рантайма `tls_placeholder` (horizon-wine/source/runtime.c ~3084: 96 потоков ждут UEvent tls_trimmed через waitSingle, затем неоставленные выходят, главный делает threadWaitForExit+threadClose) в libnx `waitObjects → _waitObjectsImpl → _waiterNodeRemove` записал в узел списка ожидания на уже освобождённом стеке другого потока. Список ожидания UEvent испорчен — замок libnx (ldaxr/stlxr + svcArbitrateLock/Unlock) или пробуждения пропустили двоих, либо поток вышел, не убрав свой узел.
4. До 0009: 9 потоков крутились в JIT, 6 ждали замок malloc (mutexLock ← rmutexLock ← _malloc_r ← heap_allocate ← __wine_unix_call_dispatcher).
5. Путь с аппаратным ускорением: всегда 0xc0000005 на ~150 с; единственный data abort — запись в 0x3ffb28fff0 главным потоком (на 16 байт ниже его 16МБ-области FEX 0x3ffb290000), тот же адрес в разных прогонах; ранее в журнале Eden — shader_recompiler/frontend/maxwell/decode.cpp «assert Invalid insn 0x20461» и «Invalid insn 0x0» на шейдерах Mesa nvc0. OpenTTD затем пишет «GPU driver crashed. Hardware acceleration disabled».

ЗАДАЧИ (три направления, доведи каждое до вывода)
A. Почему BTCpuThreadTerm завершающегося рабочего потока уничтожает состояние главного. Прочитай FEX upstream (Source/Windows/WOW64/Module.cpp: BTCpuThreadTerm, GetThreadTLS, Threads map, ThreadCreationMutex), wow64 (dlls/wow64: NtTerminateThread, RtlExitUserThread/LdrShutdownThread путь, какой хендл передаётся — 0, псевдо -2 с расширением знака, реальный), сервер Wine Autorun (horizon.c: horizon_server_get_thread_locked, разрешение 0/псевдо-хендлов, таблица хендлов, переиспользование номеров после close, состояние terminated). Найди конкретный путь, при котором ThreadTID/TLS указывают на главный поток. Реши: корень в Autorun/Wine/FEX или в поведении ядра Eden (какой svc, чем Eden отличается от Horizon).
B. Ядро Eden против Mesosphère: KThread::Exit/svcExitThread (когда хендл потока становится signaled относительно остановки гостевого кода), KHandleTable (как быстро и в каком порядке переиспользуются хендлы, есть ли счётчик в значении хендла как в Horizon), svcArbitrateLock/ArbitrateUnlock, svcSignalProcessWideKey/WaitProcessWideKeyAtomic, svcCancelSynchronization, svcWaitSynchronization, эксклюзивный монитор dynarmic arm64 (callback-only exclusives + наш check_halt_on_memory_access: может ли остановка между ldaxr и stlxr нарушить атомарность), KProcess::EnterUserException/LeaveUserException + наш 0005 (fiber-переключение внутри RunThread, миграция ядра, CurrentPhysicalCore, SaveSvcArguments). Ищи то, что объясняет сразу пункты 2, 3 и 4 (общая гипотеза: замок/пробуждение пропускает двоих или поток видит чужое состояние).
C. Путь с ускорением: что за запись в 0x3ffb28fff0 (переполнение вниз от 16МБ-области FEX? чей стек/буфер), связана ли с GPU, и что значит «Invalid insn» для шейдеров Mesa nvc0 на GM20B в Eden (неверный адрес кода шейдера / заголовок SPH / неподдержанная инструкция). Что из этого — ошибка Eden.

ЧТО СДАТЬ
- По каждому направлению: вывод, уверенность, цепочка доказательств со ссылками файл:строка (в upstream-исходниках) и на улики выше.
- Если виноват Eden: готовые патчи в виде unified diff против Eden 58c1e20 с учётом наших 0001–0009 (опиши допущения о контексте), с объяснением, почему так же ведёт себя Horizon.
- Если виноват Autorun/Wine/FEX: точное место и минимальное исправление — пометь «только сообщить».
- Список точечных диагностических записей (что логировать в Eden и где) для следующего локального прогона, если вывод не окончательный, — чтобы один прогон подтвердил или опроверг гипотезу.
- Не больше ~900 слов в итоге, главное — в первых строках.
