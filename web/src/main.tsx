import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App'
import { installScaling } from './scale'
import './styles.css'

const root = document.getElementById('root')!
installScaling(root)

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
