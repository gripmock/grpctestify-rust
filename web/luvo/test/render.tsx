import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import type { ReactElement } from 'react';

export interface Mounted {
  container: HTMLElement;
  update(element: ReactElement): void;
  unmount(): void;
  get(selector: string): HTMLElement;
  all(selector: string): HTMLElement[];
  byText(text: string): HTMLElement[];
  click(target: string | Element): void;
  type(target: string | Element, value: string): void;
  key(target: string | Element, key: string, init?: KeyboardEventInit): void;
}

export function mount(element: ReactElement): Mounted {
  const container = document.createElement('div');
  document.body.appendChild(container);
  let root: Root;
  act(() => { root = createRoot(container); root.render(element); });

  const resolve = (target: string | Element): Element => {
    if (typeof target !== 'string') return target;
    const found = container.querySelector(target) ?? document.querySelector(target);
    if (!found) throw new Error(`nothing matches ${target}`);
    return found;
  };

  return {
    container,
    update(next) { act(() => { root.render(next); }); },
    unmount() { act(() => { root.unmount(); }); container.remove(); },
    get(selector) { return resolve(selector) as HTMLElement; },
    all(selector) {
      const scope = container.contains(document.body) ? document : container;
      return [...scope.querySelectorAll<HTMLElement>(selector), ...document.querySelectorAll<HTMLElement>(selector)]
        .filter((el, i, list) => list.indexOf(el) === i);
    },
    byText(text) {
      return [...document.querySelectorAll<HTMLElement>('*')].filter(el =>
        el.children.length === 0 && el.textContent?.trim() === text);
    },
    click(target) {
      act(() => { resolve(target).dispatchEvent(new MouseEvent('click', { bubbles: true })); });
    },
    type(target, value) {
      const el = resolve(target) as HTMLInputElement;
      const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement : HTMLInputElement;
      const setter = Object.getOwnPropertyDescriptor(proto.prototype, 'value')!.set!;
      act(() => {
        setter.call(el, value);
        el.dispatchEvent(new Event('input', { bubbles: true }));
      });
    },
    key(target, key, init) {
      act(() => {
        resolve(target).dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }));
      });
    },
  };
}
