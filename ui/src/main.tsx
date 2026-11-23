import { createRoot } from 'react-dom/client';
import { App } from './App';
import { createApi } from './api/select';
import './styles.css';

const root = createRoot(document.getElementById('root')!);
void createApi().then(api => root.render(<App api={api} />));
