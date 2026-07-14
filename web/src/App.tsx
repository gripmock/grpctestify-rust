import { ModalProvider } from './components/ui/ModalContext';
import { ToastProvider } from './components/ui/ToastContext';
import { PlayLayout } from './components/layout/PlayLayout';

export default function App() {
  return (
    <ModalProvider>
      <ToastProvider>
        <PlayLayout />
      </ToastProvider>
    </ModalProvider>
  );
}
