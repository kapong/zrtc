import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { execSync } from 'child_process'

const commitHash = execSync('git rev-parse --short HEAD').toString().trim()

export default defineConfig({
  plugins: [vue()],
  define: {
    __GIT_COMMIT__: JSON.stringify(commitHash)
  }
})
