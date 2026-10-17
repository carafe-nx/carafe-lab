# NFSU с модами сама завершается в Eden: причина

**Причина в Eden: v0.2.1 создаёт процесс без ASLR.** Из-за этого каждая новая страница TLS ядра ложится в одно и
то же место. Обёртка Autorun для svcCreateThread 256 раз получает одну и ту же страницу внутри резерва гостя и
сдаётся с 0xd001. После строки `[TLS]` ни один новый поток не создаётся.

Конкретно:

- `KProcess::LoadFromMetadata` (Eden `src/core/hle/kernel/k_process.cpp:1192-1226`) не ставит
  `CreateProcessFlag::EnableAslr`. На консоли этот флаг ставит загрузчик Atmosphère
  (`stratosphere/loader/source/ldr_process_creation.cpp:440-441`).
- Без ASLR `FindFreeArea` (`k_page_table_base.cpp:1502-1550`) берёт первую свободную страницу. Новая TLS-страница
  (`k_process.cpp:658-666` → `k_thread_local_page.cpp:30`) всегда попадает в начало малой карты с 0x200000, то есть
  в резерв гостя.
- `__wrap_svcCreateThread` (Autorun `dlls/ntdll/unix/horizon.c:19700-19750`) закрывает поток и повторяет, но
  получает ту же страницу. В журнале это `tls_retries=256`. В Mesosphère место выбирается случайно
  (`kern_k_page_table_base.cpp:1258-1282`), и 256 промахов подряд практически невозможны.

Патчи Eden 0005/0009/0010, Carafe, Autorun и Wine не виноваты: на консоли тот же код получает случайную страницу и
работает.

Версии: Eden v0.2.1 (58c1e20e, зеркало eden-emulator/mirror); Autorun 636913ca; FEX 395b132f; Atmosphère 36cc9a9f;
WidescreenFixesPack dd0d6c9 (релиз `NFSUnderground.WidescreenFix.asi`, 5552000 байт, GitHash dd0d6c9);
horizon-dlls 5d6eccb. Смещения в моде — от базы образа 0x10000000.

## A. Цепочка

1. `hold_thread_local_pages` (`horizon-wine/source/runtime.c:3095-3140`) успела создать 9 потоков-заглушек на
   2 страницах, 2 оставила. Свободно 7 мест TLS, больше не будет.
2. Один поток Windows на Horizon занимает два потока ядра: сам поток (`dlls/ntdll/unix/thread.c:1396`) и поток
   соединения сервера (`horizon.c:11583`). Когда 7 мест кончились, каждый NtCreateThreadEx падает.
3. Мод: `dllmain.cpp:118` вызывает `executeAllAsync()`, там `std::async(launch::async)`
   (`includes/stdafx.h:1173`). Дальше `_Schedule_chore` (+0x5e780) вызывает CreateThreadpoolWork. В Wine у пула нет
   рабочих потоков, создание первого падает (`dlls/ntdll/threadpool.c:1887-1888, 1208-1222`), функция возвращает
   NULL.
4. `schedule` (+0x289d0) бросает `Concurrency::invalid_operation("Fail to schedule the chore!")` (+0x28a41).
5. Его ловит `catch(...)` внутри PPL, `_Task_impl_base::_ScheduleTask` (+0x27f30, FuncInfo 0x101351d4, обработчик
   +0x27fee). Задача отменяется, Init идёт дальше.
6. Раскрутка: CatchIt (+0x6201d) → `_UnwindNestedFrames` (+0x61240) → RtlUnwind → NtContinue
   (`dlls/ntdll/signal_i386.c:356`) → BTCpuSetContext (`dlls/wow64/syscall.c:688`). Отсюда eip=+0x6126e и eax=0
   (это retval RtlUnwind).

### Почему это именно это исключение — совпадение до байта

- ebx=0096e524 — указатель на запись исключения (CatchIt кладёт ebx в стек перед вызовом), edi=edx=0096e930 —
  узел регистрации.
- Для этого пути начало кадра `_ScheduleTask` = 0096e93c. Кадры schedule, `_CxxThrowException` (+0x60490) и
  kernelbase!RaiseException (0x58, сверено по двоичным файлам horizon-dlls) дают Esp контекста 0096e85c.
- wow64 кладёт запись по адресу `((Esp−0x58−0x2E7)&~3)+8` (`dlls/wow64/syscall.c:252`,
  `dlls/ntdll/exception.c:725`) = 0096e524.

### «Мод раскрутил стек, и процесс вышел» — неверно

- Под FEX область CPU пишется только в BTCpuSetContext/GetContext (FEX `Source/Windows/WOW64/Module.cpp:292-302,
  725-760`); системные вызовы её не обновляют (`:452-485`). `[BOX64EXIT]` читает именно её
  (`dlls/ntdll/unix/process.c:984-985`). Значит, это последняя смена контекста потока 4, а не место выхода.
- Стек это подтверждает: в ячейках кадра должны быть 0x162e126f, 0x0096e358 и 0x0096e930, а в журнале 0x00990040,
  2 и 0x00eeaed0. Кадр перезаписан кодом, который работал позже.

### Выход с кодом 0

tid=4 — главный поток (`horizon.c:6621`). `verdict=PASS` требует running=1 и столько же объектов-потоков, сколько
было до первого NtCreateThreadEx (`horizon.c:14032-14039`, `thread.c:1480`). То есть в момент выхода не жил ни один
поток программы.

**Не доказано:** какая инструкция вызвала ExitProcess(0). Под FEX этот журнал её не содержит; как получить — в
разделе D.

## Наблюдения 2–6

- **2:** игра закрывается сама, без падения, другие её потоки не созданы.
- **3:** мод и xinput1_4 грузятся; значения в ini — числа, ReadInteger не бросает. `modloader` c0000135 — папки нет,
  это норма.
- **4:** `[THREAD]` и `[TLS]` — не норма 32-битного процесса, а следствие отсутствия ASLR в Eden. Связь с
  завершением прямая.
- **5:** RtlUnwind — это catch(...) внутри PPL, описанный выше.
- **6:** 0x4b0 и 0x480 — мусор в освободившейся части стека, к экрану отношения нет. 1616e671 (ZPatchU1+0xe671) —
  след кода, работавшего позже: ZPatchU1 грузится после мода.

## B. Кандидаты

- **Регионы 32-битного процесса:** совпадают (Eden `k_page_table_base.cpp:249-263`, Mesosphère `:187-205`: код,
  стек и kernel map = 0x200000–0x40000000). Отличие одно — ASLR. **Да, это причина.**
- **Доставка исключений, 0005/0009/0010:** **нет.** Исключение программное (RaiseException → NtRaiseException →
  wow64), ядро его не видит.
  - Кадр 0005 совпадает с Mesosphère: sp=PLR+0x140, x0=тип, x1=PLR+0x148, pc=точка входа, psr=0; возврат
    восстанавливает x0–x8, lr, sp, pc, psr&0xF0000000 (`kern_exception_handlers.cpp:149-252, 395-430`).
  - Добавленный SaveSvcArguments не затирает восстановленные x0–x7.
  - Расхождения есть (синтетический ESR: ISV=0, у prefetch IFSC=0; доставка только 64-битному процессу), но на
    этот путь они не влияют.
- **Права страниц, VirtualProtect, PAGE_GUARD, запись в код игры:** **нет.** Обработчик пошёл по ветке
  C++-исключения 0xE06D7363, нарушения доступа в цепочке нет.
- **Что мод спрашивает на старте:** **нет**, кроме потоков. Шаблон найден сразу, Init вызывается напрямую. Экран и
  звук — только числа в памяти, XInput загрузился, ini читается без ошибок.

## C. Почему не Carafe, не Autorun и не Wine

Мод, Wine, FEX и рантайм на консоли проходят тот же путь. Там TLS-страница ложится случайно, цикл повторов находит
место, CreateThreadpoolWork получает поток. Расходится только `FindFreeArea` без ASLR в Eden. Это не наши патчи
0005/0009/0010.

## D. Патч Eden

```diff
--- a/src/core/hle/kernel/k_process.cpp
+++ b/src/core/hle/kernel/k_process.cpp
@@ -1202,6 +1202,10 @@ Result KProcess::LoadFromMetadata(const FileSys::ProgramMetadata& metadata, std:
         flag |= Svc::CreateProcessFlag::Is64Bit;
     }
 
+    // As on the console: ldr gives every process ASLR unless pm asks for DisableAslr, and
+    // Mesosphere's FindFreeArea then places thread-local pages, stacks and regions at random.
+    flag |= Svc::CreateProcessFlag::EnableAslr;
+
     // Set the address space type and code address.
     switch (metadata.GetAddressSpaceType()) {
     case FileSys::ProgramAddressSpaceType::Is39Bit:
```

Хунк не задевает строки патча 0008.

**Проверка за один прогон с патчем:**

- пропадут все `[THREAD] create failed … tls_retries=256`;
- строка `[TLS]` покажет до 96 заглушек и rc≠0xd001;
- игра дойдёт до графики (Vulkan, DXVK), строки `[EXIT]` через ~29 с не будет.

**Без пересборки:** в полном журнале Eden между `[TLS]` и строкой `[NXLDR] … ZPatchU1` должна быть ещё хотя бы одна
`[THREAD] create failed … tls_retries=256` с threads>15. Если её нет — CreateThreadpoolWork упал по другой причине,
и звено 2 цепочки неверно.

**Точное место ExitProcess(0)**, если оно нужно: тот же NSP без патча с `cpu = box64`. У Box64 шлюз системных
вызовов публикует контекст, и `[BOX64EXIT]` покажет настоящий eip.

**Побочные эффекты патча:** раскладка станет случайной, как на консоли. Объём `[VA] early guest reservations` и
адрес `[USD]` будут меняться от запуска к запуску. Примерно в 1,2 % запусков загрузчик увидит занятый 0x400000 и
вызовет RestartProgram — работает ли он в Eden, не проверено.
