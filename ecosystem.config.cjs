module.exports = {
  apps: [
    {
      name: 'dashboard-api',
      cwd: '/ssd_pool/server-dashboard/api',
      script: 'npx',
      args: 'nodemon src/index.js',
      watch: false,
      env: {
        NODE_ENV: 'development'
      }
    },
    {
      name: 'dashboard-web',
      cwd: '/ssd_pool/server-dashboard/web',
      script: 'npx',
      args: 'vite',
      watch: false,
      env: {
        NODE_ENV: 'development'
      }
    }
  ]
};
