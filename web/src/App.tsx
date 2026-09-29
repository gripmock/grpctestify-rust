import { Crash } from './components/ui/Crash';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { PlayLayout } from './components/layout/PlayLayout';

export default function App() {
  return (
    <Crash>
      <ModalProvider>
        <ToastProvider>
          <PlayLayout />
        </ToastProvider>
      </ModalProvider>
    </Crash>
  );
}
