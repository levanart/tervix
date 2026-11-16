# ADR-010: логическая геометрия и направленные команды tiling

Статус: **предложено**, требует ревью. Дата: 2026-11-08.
Дополняет ADR-009; публичные API начального среза остаются предварительными.

## Контекст

Одного ratio недостаточно для TILE-003: глубокие split могут сделать панели
нулевыми или недоступными. TILE-002/005 требуют общую семантику направленного
focus/move/swap/resize и истории фокуса до подключения разных input adapters.

## Предлагаемое решение

Новых crates и зависимостей нет. Геометрия и навигация — чистые функции в core,
история фокуса — состояние одного tab controller в application.

### Геометрия

- `Size` — положительные целые логические единицы `u32`; не physical DPI pixels.
  Преобразование DPI, терминальных ячеек и toolkit coordinates — задача UI.
- Общий minimum для каждого tile включает будущие header/border. Per-view minima
  остаются расширением вместе с registry настоящих view instances.
- Минимум поддерева: сумма вдоль axis split и максимум поперёк него.
  Переполнение `u32` — типизированная ошибка, не wrap или panic.
- Canvas равен покомпонентному максимуму viewport и минимума корня. На маленьком
  viewport возвращаются overflow flags; будущий UI **обязан** предоставить pan/
  scroll и раскрывать focused tile, а не молча обрезать недоступные панели.
- Rectangles half-open, без зазоров; остаток integer division получает второй
  ребёнок. Allocation clamp учитывает минимумы обоих поддеревьев.
- Projection не переписывает ratio: при увеличении окна восстанавливаются
  запрошенные пропорции. Нет side effects, изменения identity или focus.
- `Rect` — DTO, но `LayoutGeometry` нельзя собрать из произвольных rectangles:
  neighbor lookup работает только с проверенной полной projection.

### Направленные действия

`LayoutCommand::Navigate` содержит typed action, viewport и minimum, чтобы UI,
клавиатура и palette использовали один алгоритм и одинаковые logical dimensions.

- Neighbor должен лежать полностью с нужной стороны и иметь положительное
  поперечное пересечение. Касание угла не считается; нет wrap на краях.
- Порядок выбора: forward edge gap, затем расстояние поперечных центров,
  затем depth-first порядок дерева. Не зависит от UUID или случайности.
- Focus меняет только активный ID. Swap обменивает целые пары pane/view, сохраняя
  focus на прежней identity, а не прямоугольнике.
- Move выбирает target по исходной geometry, удаляет active leaf, схлопывает
  старый parent, делит target 50/50 и помещает active на указанной стороне target.
  Это структурное перемещение, а не псевдоним swap. Размеры соседей могут меняться.
- Resize толкает наружу ближайшую ancestor boundary на выбранной стороне,
  подходящую по axis и стороне child. Step 1..=8000 basis points; результат
  clamp 1000..=9000. На границе clamp — no-op без новой записи undo.
  Pixel/logical minima обеспечиваются projection; на минимальном canvas push
  может менять сохранённый ratio без видимого роста. Resize не требует neighbor
  measurement: viewport/минимум не влияют на выбор границы в дереве.
- Нет соседа/границы, некорректный step или превышение depth limit — ошибка без
  частичного изменения layout и histories. Move сохраняет все pane/view ID.

### История фокуса

Controller хранит до 32 уникальных ранее активных reachable pane ID без current
focus. `FocusPrevious` переключает на последний доступный ID (повторение toggles).
Закрытие active предпочитает MRU, затем доменный depth-first fallback. Закрытие
неактивной панели не меняет focus. Удалённые ID удаляются из MRU, включая undo split.
History фокуса не является snapshot topology; undo восстанавливает точный layout
и его focus, затем reconciles MRU. Оно не восстанавливает забытые MRU записи.

Только topology/ratio изменения занимают layout undo; direct/directional focus,
FocusPrevious и no-op не занимают. Ошибочные команды не меняют ни одну history.

## Границы и проверка

Domain API можно применять для любого pane и нескольких однотипных view, без
PTY/Docker/sysinfo в алгоритме. Detach и restore не реализованы: операции пока
локальны одному tab. Session continuity ещё предстоит проверить на реальных PTY.
Нет GUI, shortcuts, pointer dragging, focus highlight и фактического scroll UI;
поэтому TILE-002/003/005 целиком ещё не закрыты.

Тесты: exact odd-size rectangles, clamp и overflow, предельные `u32` координаты,
случайные деревья с полной неперекрывающейся площадью и минимальными размерами;
direction ties/corners/edges, move vs swap, resize ancestor, identity preservation,
MRU close/undo/bounds и property-based sequences. Обязательные quality gates
выполняются на Linux; Windows/GUI acceptance остаются отдельной проверкой.
