import { useRef, useEffect, useState, useCallback, useMemo } from 'react';
import { useAppContext } from '../context';
import { useTodos, addTodo, updateTodo, removeTodo } from '../hooks';
import type { TodoItem, TodoTree } from '../types';

interface FlatItem {
  id: string;
  item: TodoItem;
  depth: number;
  parentId: string | null;
}

/** DFS flatten the tree for rendering and j/k navigation */
function flattenTree(tree: TodoTree): FlatItem[] {
  const result: FlatItem[] = [];

  function visit(ids: string[], depth: number, parentId: string | null) {
    for (const id of ids) {
      const item = tree.items[id];
      if (!item) continue;
      result.push({ id, item, depth, parentId });
      if (item.children.length > 0) {
        visit(item.children, depth + 1, id);
      }
    }
  }

  visit(tree.root_ids, 0, null);
  return result;
}

/** Find the parent id of an item in the tree */
function findParentId(tree: TodoTree, itemId: string): string | null {
  for (const [id, item] of Object.entries(tree.items)) {
    if (item && item.children.includes(itemId)) return id;
  }
  return null;
}

/** Find the previous sibling of an item in its sibling list */
function findPreviousSibling(tree: TodoTree, itemId: string): string | null {
  const parentId = findParentId(tree, itemId);
  const siblings = parentId ? (tree.items[parentId]?.children ?? []) : tree.root_ids;
  const idx = siblings.indexOf(itemId);
  return idx > 0 ? siblings[idx - 1] : null;
}

export function TodoPanel() {
  const tree = useTodos();
  const {
    focusedPanel,
    setFocusedPanel,
    selectedTodoId,
    setSelectedTodoId,
    navigateTodos,
  } = useAppContext();

  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState('');
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const selectedRef = useRef<HTMLDivElement>(null);
  const isFocused = focusedPanel === 'todos';

  const flatItems = useMemo(() => flattenTree(tree), [tree]);
  const flatIds = useMemo(() => flatItems.map(f => f.id), [flatItems]);

  // Auto-focus input when entering edit mode
  useEffect(() => {
    if (editingId && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editingId]);

  // Scroll selected item into view
  useEffect(() => {
    if (selectedTodoId && selectedRef.current) {
      selectedRef.current.scrollIntoView({ block: 'nearest' });
    }
  }, [selectedTodoId]);

  // Clear delete confirmation after timeout
  useEffect(() => {
    if (deleteConfirmId) {
      const timer = setTimeout(() => setDeleteConfirmId(null), 2000);
      return () => clearTimeout(timer);
    }
  }, [deleteConfirmId]);

  const startEditing = useCallback((id: string, text: string) => {
    setEditingId(id);
    setEditText(text);
  }, []);

  const saveEdit = useCallback(async () => {
    if (!editingId) return;
    const trimmed = editText.trim();
    if (trimmed) {
      await updateTodo(editingId, { text: trimmed });
    }
    setEditingId(null);
    setEditText('');
  }, [editingId, editText]);

  const cancelEdit = useCallback(() => {
    setEditingId(null);
    setEditText('');
  }, []);

  const insertItem = useCallback(async () => {
    // Insert new item after current selection, at same level
    const selectedFlat = flatItems.find(f => f.id === selectedTodoId);
    const parentId = selectedFlat?.parentId ?? null;
    const afterId = selectedTodoId;

    const newTree = await addTodo('', parentId, afterId);
    // Find the newly created item (the one not in the old tree)
    const oldIds = new Set(Object.keys(tree.items));
    const newId = Object.keys(newTree.items).find(id => !oldIds.has(id));
    if (newId) {
      setSelectedTodoId(newId);
      startEditing(newId, '');
    }
  }, [flatItems, selectedTodoId, tree.items, setSelectedTodoId, startEditing]);

  const indentItem = useCallback(async () => {
    if (!selectedTodoId) return;
    const prevSibling = findPreviousSibling(tree, selectedTodoId);
    if (!prevSibling) return; // Can't indent first child
    // Move under previous sibling, as last child
    const prevChildren = tree.items[prevSibling]?.children ?? [];
    const afterId = prevChildren.length > 0 ? prevChildren[prevChildren.length - 1] : null;
    await updateTodo(selectedTodoId, { parent_id: prevSibling, after_id: afterId });
  }, [selectedTodoId, tree]);

  const outdentItem = useCallback(async () => {
    if (!selectedTodoId) return;
    const parentId = findParentId(tree, selectedTodoId);
    if (!parentId) return; // Already at root
    const grandparentId = findParentId(tree, parentId);
    // Move after parent, at grandparent level
    await updateTodo(selectedTodoId, { parent_id: grandparentId ?? '', after_id: parentId });
  }, [selectedTodoId, tree]);

  // Keyboard handling
  useEffect(() => {
    if (!isFocused) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't handle keys when in edit mode (input handles those)
      if (editingId) return;
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      switch (e.key) {
        case 'j':
        case 'ArrowDown':
          e.preventDefault();
          navigateTodos('down', flatIds);
          break;
        case 'k':
        case 'ArrowUp':
          e.preventDefault();
          navigateTodos('up', flatIds);
          break;
        case 'Enter':
          e.preventDefault();
          if (selectedTodoId) {
            const item = tree.items[selectedTodoId];
            if (item) startEditing(selectedTodoId, item.text);
          }
          break;
        case 'o':
          e.preventDefault();
          insertItem();
          break;
        case '>':
          e.preventDefault();
          indentItem();
          break;
        case '<':
          e.preventDefault();
          outdentItem();
          break;
        case ' ':
          e.preventDefault();
          if (selectedTodoId) {
            const item = tree.items[selectedTodoId];
            if (item) updateTodo(selectedTodoId, { checked: !item.checked });
          }
          break;
        case 'd':
          e.preventDefault();
          if (selectedTodoId) {
            if (deleteConfirmId === selectedTodoId) {
              removeTodo(selectedTodoId);
              // Select next or previous item
              const idx = flatIds.indexOf(selectedTodoId);
              const nextId = flatIds[idx + 1] ?? flatIds[idx - 1] ?? null;
              setSelectedTodoId(nextId);
              setDeleteConfirmId(null);
            } else {
              setDeleteConfirmId(selectedTodoId);
            }
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isFocused, editingId, flatIds, selectedTodoId, tree, navigateTodos, startEditing, insertItem, indentItem, outdentItem, deleteConfirmId, setSelectedTodoId]);

  return (
    <div
      className={`border-t border-gray-200 bg-gray-50 flex flex-col ${isFocused ? 'ring-1 ring-blue-300 ring-inset' : ''}`}
      style={{ height: '200px' }}
      onClick={() => setFocusedPanel('todos')}
    >
      {/* Header */}
      <div className="flex items-center px-3 py-1.5 border-b border-gray-200 bg-gray-100">
        <span className="text-xs font-semibold text-gray-500 uppercase tracking-wide">Todos</span>
        <span className="ml-2 text-xs text-gray-400">
          {flatItems.filter(f => !f.item.checked).length} remaining
        </span>
        <span className="flex-1" />
        <span className="text-xs text-gray-400">
          {isFocused && 'j/k nav  o new  >/< indent  space check  d delete'}
        </span>
      </div>

      {/* Tree */}
      <div className="flex-1 overflow-y-auto">
        {flatItems.length === 0 ? (
          <div className="flex items-center justify-center h-full text-gray-400 text-sm">
            No todos yet. {isFocused ? 'Press o to create one.' : ''}
          </div>
        ) : (
          flatItems.map(({ id, item, depth }) => {
            const isSelected = selectedTodoId === id;
            const isEditing = editingId === id;
            const isDeleteConfirm = deleteConfirmId === id;

            return (
              <div
                key={id}
                ref={isSelected ? selectedRef : undefined}
                className={`flex items-center py-0.5 pr-2 cursor-pointer text-sm ${
                  isSelected && isFocused
                    ? 'bg-blue-100'
                    : isSelected
                      ? 'bg-blue-50'
                      : 'hover:bg-gray-100'
                } ${item.checked ? 'opacity-50' : ''}`}
                style={{ paddingLeft: `${depth * 20 + 8}px` }}
                onClick={() => {
                  setSelectedTodoId(id);
                  setFocusedPanel('todos');
                }}
                onDoubleClick={() => startEditing(id, item.text)}
              >
                {/* Checkbox */}
                <button
                  className={`flex-shrink-0 w-4 h-4 rounded border mr-2 flex items-center justify-center ${
                    item.checked
                      ? 'bg-blue-500 border-blue-500 text-white'
                      : 'border-gray-300 hover:border-blue-400'
                  }`}
                  onClick={(e) => {
                    e.stopPropagation();
                    updateTodo(id, { checked: !item.checked });
                  }}
                >
                  {item.checked && (
                    <svg className="w-3 h-3" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={3}>
                      <path strokeLinecap="round" strokeLinejoin="round" d="M5 13l4 4L19 7" />
                    </svg>
                  )}
                </button>

                {/* Text or input */}
                {isEditing ? (
                  <input
                    ref={inputRef}
                    type="text"
                    value={editText}
                    onChange={(e) => setEditText(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') {
                        e.preventDefault();
                        saveEdit();
                      } else if (e.key === 'Escape') {
                        e.preventDefault();
                        cancelEdit();
                      }
                    }}
                    onBlur={saveEdit}
                    className="flex-1 text-sm bg-white border border-blue-300 rounded px-1 py-0 outline-none"
                  />
                ) : (
                  <span className={`flex-1 truncate ${item.checked ? 'line-through text-gray-400' : ''}`}>
                    {item.text || <span className="text-gray-300 italic">empty</span>}
                  </span>
                )}

                {/* Delete confirmation */}
                {isDeleteConfirm && (
                  <span className="ml-2 text-xs text-red-500 flex-shrink-0">
                    press d again
                  </span>
                )}
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}
