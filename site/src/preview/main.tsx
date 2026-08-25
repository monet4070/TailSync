import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './preview-page.css'
import { PreviewApp } from './PreviewApp.tsx'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewApp />
  </StrictMode>,
)
