<template>
  <div class="gctf-generator">
    <div class="generator-header">
      <h2>🚀 Interactive .gctf Generator</h2>
      <p class="description">Create test files for any gRPC service - unary, streaming, or advanced patterns</p>
    </div>

    <div class="examples-grid">
      <div class="example-card" @click="loadExample('unary')">
        <h4>🔄 Unary Request</h4>
        <p>Simple request-response pattern</p>
        <small>Perfect for CRUD operations, authentication</small>
      </div>
      <div class="example-card" @click="loadExample('streaming')">
        <h4>📊 Server Streaming</h4>
        <p>Multiple response validation</p>
        <small>Payment processing, status updates</small>
      </div>
      <div class="example-card" @click="loadExample('tls')">
        <h4>🔒 TLS Secure</h4>
        <p>Encrypted communication</p>
        <small>Production-grade security testing</small>
      </div>
      <div class="example-card" @click="loadExample('error')">
        <h4>⚠️ Error Testing</h4>
        <p>Invalid input scenarios</p>
        <small>Error codes and messages validation</small>
      </div>
    </div>

    <div class="form-section">
      <h3>🔗 Connection Settings</h3>
      <div class="form-row">
        <div class="form-group">
          <label>Server Address</label>
          <input v-model="address" type="text" placeholder="localhost:50051">
          <small>Format: host:port</small>
        </div>
        <div class="form-group">
          <label>gRPC Method</label>
          <input v-model="endpoint" type="text" placeholder="package.Service/Method">
          <small>Example: user.UserService/CreateUser</small>
        </div>
      </div>
    </div>

    <div class="form-section">
      <h3>📝 Request Configuration</h3>
      <div class="form-group">
        <label>Request JSON</label>
        <textarea v-model="request" rows="8" placeholder="Enter your gRPC request payload..."></textarea>
        <small>💡 Use &quot;*&quot; for dynamic values that will be generated</small>
      </div>
    </div>

    <div class="form-section">
      <h3>✅ Response Validation</h3>
      <div class="validation-type">
        <label>
          <input v-model="validationType" type="radio" value="response">
          Expected Response Structure
        </label>
        <label>
          <input v-model="validationType" type="radio" value="assertions">
          Custom Assertions (jq)
        </label>
        <label>
          <input v-model="validationType" type="radio" value="multiple">
          Multiple Response Validation (Streaming)
        </label>
      </div>

      <div v-if="validationType === 'response'" class="form-group">
        <label>Expected Response</label>
        <textarea v-model="response" rows="8" placeholder="Expected response structure..."></textarea>
        <small>💡 Partial matching - only specified fields are validated</small>
      </div>

      <div v-if="validationType === 'assertions'" class="form-group">
        <label>jq Assertions</label>
        <textarea v-model="assertions" rows="6" placeholder=".user.id | length > 0&#10;.success == true&#10;.timestamp | test('^[0-9]{4}-')"></textarea>
        <small>💡 Each line is a separate assertion</small>
      </div>

      <div v-if="validationType === 'multiple'" class="form-group">
        <label>Multiple Response Stages</label>
        <div class="multiple-asserts-container">
          <div v-for="(assert, index) in multipleAsserts" :key="index" class="assert-stage">
            <div class="assert-header">
              <span>Stage {{ index + 1 }}</span>
              <button v-if="multipleAsserts.length > 1" @click="removeAssertStage(index)" type="button" class="remove-btn">Remove</button>
            </div>
            <textarea v-model="multipleAsserts[index]" rows="3" placeholder=".status == 'PROCESSING'&#10;.progress >= 0"></textarea>
          </div>
        </div>
        <button @click="addAssertStage()" type="button" class="add-stage-btn">+ Add Stage</button>
        <small>💡 Perfect for server streaming with status updates</small>
      </div>
    </div>

    <div class="form-section">
      <h3>🔒 Security & Advanced</h3>
      <div class="form-group">
        <label>
          <input v-model="enableTls" type="checkbox">
          Enable TLS/mTLS
        </label>
      </div>
      
      <div v-if="enableTls" class="tls-config">
        <div class="form-row">
          <div class="form-group">
            <label>CA Certificate Path</label>
            <input v-model="caCert" type="text" placeholder="./certs/ca-cert.pem">
          </div>
          <div class="form-group">
            <label>Server Name</label>
            <input v-model="serverName" type="text" placeholder="api.example.com">
          </div>
        </div>
        <div class="form-row">
          <div class="form-group">
            <label>Client Certificate (mTLS)</label>
            <input v-model="clientCert" type="text" placeholder="./certs/client-cert.pem">
          </div>
          <div class="form-group">
            <label>Client Key (mTLS)</label>
            <input v-model="clientKey" type="text" placeholder="./certs/client-key.pem">
          </div>
        </div>
      </div>

      <div class="form-group">
        <label>
          <input v-model="enableError" type="checkbox">
          Test Error Response
        </label>
      </div>
      
      <div v-if="enableError" class="form-group">
        <label>Expected Error</label>
        <textarea v-model="errorResponse" rows="4" placeholder='{"code": 5, "message": "User not found"}'></textarea>
      </div>

      <div class="form-group">
        <label>Request Headers (Optional)</label>
        <textarea v-model="headers" rows="3" placeholder='{"authorization": "Bearer token", "x-api-key": "secret"}'></textarea>
      </div>

      <div class="form-row">
        <div class="form-group">
          <label>Timeout (seconds)</label>
          <input v-model="timeout" type="number" placeholder="30" min="1" max="300">
        </div>
        <div class="form-group">
          <label>Retries</label>
          <input v-model="retries" type="number" placeholder="3" min="0" max="10">
        </div>
      </div>
    </div>

    <div class="generator-actions">
      <button @click="copyToClipboard()" class="primary-btn">📋 Copy to Clipboard</button>
      <button @click="downloadTest()" class="primary-btn">💾 Download .gctf</button>
      <button @click="clearForm()" class="secondary-btn">🗑️ Clear Form</button>
    </div>

    <div class="generator-output">
      <div class="output-header">
        <h3>Generated .gctf File</h3>
      </div>
      <pre><code class="language-php">{{ generatedContent }}</code></pre>
    </div>
  </div>
</template>

<script setup>
import { ref, computed } from 'vue'

const address = ref('localhost:50051')
const endpoint = ref('')
const request = ref('')
const response = ref('')
const assertions = ref('')
const enableTls = ref(false)
const caCert = ref('./../server/tls/ca-cert.pem')
const clientCert = ref('./../server/tls/client-cert.pem')
const clientKey = ref('./../server/tls/client-key.pem')
const serverName = ref('localhost')
const enableError = ref(false)
const errorResponse = ref('')
const headers = ref('')
const timeout = ref('')
const retries = ref('')

const validationType = ref('response')
const multipleAsserts = ref(['.status == "PROCESSING"\n.progress >= 0'])

const generatedContent = computed(() => {
  if (!endpoint.value || !request.value) {
    return 'Please provide at least an endpoint and request payload to generate a .gctf file.'
  }

  let content = ''
  
  // Address
  content += `--- ADDRESS ---\n${address.value}\n\n`
  
  // TLS
  if (enableTls.value) {
    content += `--- TLS ---\n`
    if (caCert.value) content += `ca_cert: ${caCert.value}\n`
    if (clientCert.value) content += `cert: ${clientCert.value}\n`
    if (clientKey.value) content += `key: ${clientKey.value}\n`
    if (serverName.value) content += `server_name: ${serverName.value}\n`
    content += '\n'
  }
  
  // Endpoint
  content += `--- ENDPOINT ---\n${endpoint.value}\n\n`
  
  // Headers
  if (headers.value) {
    content += `--- HEADERS ---\n${headers.value}\n\n`
  }
  
  // Request
  content += `--- REQUEST ---\n${request.value}\n\n`
  
  // Response/Error/Assertions
  if (enableError.value && errorResponse.value) {
    content += `--- ERROR ---\n${errorResponse.value}\n\n`
  } else if (validationType.value === 'response' && response.value) {
    content += `--- RESPONSE ---\n${response.value}\n\n`
  } else if (validationType.value === 'assertions' && assertions.value) {
    content += `--- ASSERT ---\n${assertions.value}\n\n`
  } else if (validationType.value === 'multiple') {
    multipleAsserts.value.forEach(assert => {
      if (assert.trim()) {
        content += `--- ASSERTS ---\n${assert}\n\n`
      }
    })
  }
  
  // Options
  if (timeout.value || retries.value) {
    content += `--- OPTIONS ---\n`
    if (timeout.value) content += `timeout: ${timeout.value}s\n`
    if (retries.value) content += `retries: ${retries.value}\n`
    content += '\n'
  }
  
  return content
})

function loadExample(type) {
  switch (type) {
    case 'unary':
      address.value = 'localhost:50051'
      endpoint.value = 'user.UserService/CreateUser'
      request.value = JSON.stringify({
        username: "john_doe",
        email: "john@example.com",
        password: "secure123",
        profile: {
          first_name: "John",
          last_name: "Doe"
        }
      }, null, 2)
      response.value = JSON.stringify({
        user: {
          id: "*",
          username: "john_doe",
          email: "john@example.com",
          is_active: true
        },
        success: true
      }, null, 2)
      validationType.value = 'response'
      enableTls.value = false
      enableError.value = false
      break
      
    case 'streaming':
      address.value = 'localhost:50051'
      endpoint.value = 'payment.PaymentService/ProcessPayment'
      request.value = JSON.stringify({
        payment_id: "pay_12345",
        amount: 99.99,
        currency: "USD"
      }, null, 2)
      validationType.value = 'multiple'
      multipleAsserts.value = [
        '.status == "VALIDATION"\n.progress_percentage <= 20',
        '.status == "PROCESSING"\n.progress_percentage >= 20\n.progress_percentage <= 80',
        '.status == "COMPLETED"\n.progress_percentage == 100\n.success == true'
      ]
      enableTls.value = false
      enableError.value = false
      break
      
    case 'tls':
      address.value = 'localhost:50051'
      endpoint.value = 'secure.SecureService/AuthenticateUser'
      request.value = JSON.stringify({
        username: "admin",
        password: "secret123"
      }, null, 2)
      response.value = JSON.stringify({
        token: "*",
        expires_at: "*",
        user: {
          role: "admin"
        }
      }, null, 2)
      enableTls.value = true
      validationType.value = 'response'
      enableError.value = false
      break
      
    case 'error':
      address.value = 'localhost:50051'
      endpoint.value = 'user.UserService/GetUser'
      request.value = JSON.stringify({
        user_id: "invalid_user_id"
      }, null, 2)
      enableError.value = true
      errorResponse.value = JSON.stringify({
        code: 5,
        message: "User not found"
      }, null, 2)
      enableTls.value = false
      validationType.value = 'response'
      break
  }
}

function addAssertStage() {
  multipleAsserts.value.push('.status == "NEW_STAGE"\n.progress >= 50')
}

function removeAssertStage(index) {
  if (multipleAsserts.value.length > 1) {
    multipleAsserts.value.splice(index, 1)
  }
}

function copyToClipboard() {
  navigator.clipboard.writeText(generatedContent.value).then(() => {
    // You could add a toast notification here
    console.log('Copied to clipboard!')
  })
}

function downloadTest() {
  const filename = (endpoint.value || 'test').replace(/[^a-zA-Z0-9]/g, '_').toLowerCase() + '.gctf'
  const blob = new Blob([generatedContent.value], { type: 'text/plain' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}

function clearForm() {
  address.value = 'localhost:50051'
  endpoint.value = ''
  request.value = ''
  response.value = ''
  assertions.value = ''
  enableTls.value = false
  caCert.value = './../server/tls/ca-cert.pem'
  clientCert.value = './../server/tls/client-cert.pem'
  clientKey.value = './../server/tls/client-key.pem'
  serverName.value = 'localhost'
  enableError.value = false
  errorResponse.value = ''
  headers.value = ''
  timeout.value = ''
  retries.value = ''
  validationType.value = 'response'
  multipleAsserts.value = ['.status == "PROCESSING"\n.progress >= 0']
}
</script>

<style scoped>
.gctf-generator {
  max-width: 1000px;
  margin: 2rem auto;
  padding: 0 1rem;
}

.generator-header {
  text-align: center;
  margin-bottom: 2rem;
  padding: 2rem;
  background: linear-gradient(135deg, var(--vp-c-brand) 0%, var(--vp-c-brand-dark) 100%);
  border-radius: 12px;
  color: white;
}

.generator-header h2 {
  margin: 0 0 0.5rem 0;
  font-size: 2rem;
}

.description {
  margin: 0;
  opacity: 0.9;
}

.examples-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
  gap: 1rem;
  margin: 2rem 0;
}

.example-card {
  background: var(--vp-c-bg-soft);
  border: 1px solid var(--vp-c-border);
  border-radius: 8px;
  padding: 1.5rem;
  cursor: pointer;
  transition: all 0.2s;
}

.example-card:hover {
  border-color: var(--vp-c-brand);
  transform: translateY(-2px);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
}

.example-card h4 {
  margin: 0 0 0.5rem 0;
  color: var(--vp-c-text-1);
}

.example-card p {
  margin: 0 0 0.5rem 0;
  color: var(--vp-c-text-2);
}

.example-card small {
  color: var(--vp-c-text-3);
}

.form-section {
  background: var(--vp-c-bg-soft);
  border-radius: 8px;
  padding: 1.5rem;
  margin-bottom: 1.5rem;
  border: 1px solid var(--vp-c-border);
}

.form-section h3 {
  margin-top: 0;
  color: var(--vp-c-text-1);
  border-bottom: 2px solid var(--vp-c-border);
  padding-bottom: 0.5rem;
}

.form-row {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 1rem;
}

.form-group {
  margin-bottom: 1rem;
}

.form-group label {
  display: block;
  margin-bottom: 0.5rem;
  font-weight: 500;
  color: var(--vp-c-text-1);
}

.form-group input,
.form-group textarea {
  width: 100%;
  padding: 0.75rem;
  border: 1px solid var(--vp-c-border);
  border-radius: 6px;
  font-family: var(--vp-font-family-mono);
  background: var(--vp-c-bg);
  color: var(--vp-c-text-1);
  transition: border-color 0.2s;
  box-sizing: border-box;
}

.form-group input:focus,
.form-group textarea:focus {
  outline: none;
  border-color: var(--vp-c-brand);
}

.form-group small {
  display: block;
  margin-top: 0.25rem;
  color: var(--vp-c-text-3);
  font-size: 0.875rem;
}

.validation-type {
  display: flex;
  gap: 1.5rem;
  margin-bottom: 1rem;
  flex-wrap: wrap;
}

.validation-type label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  cursor: pointer;
}

.assert-stage {
  background: var(--vp-c-bg);
  border: 1px solid var(--vp-c-border);
  border-radius: 6px;
  padding: 1rem;
  margin-bottom: 1rem;
}

.assert-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 0.5rem;
}

.assert-header span {
  font-weight: 500;
  color: var(--vp-c-text-1);
}

.remove-btn {
  background: #ef4444;
  color: white;
  border: none;
  padding: 0.25rem 0.5rem;
  border-radius: 4px;
  cursor: pointer;
  font-size: 0.75rem;
}

.add-stage-btn {
  background: var(--vp-c-brand);
  color: white;
  border: none;
  padding: 0.5rem 1rem;
  border-radius: 6px;
  cursor: pointer;
  font-weight: 500;
}

.generator-actions {
  display: flex;
  gap: 1rem;
  margin: 2rem 0;
  justify-content: center;
  flex-wrap: wrap;
}

.primary-btn {
  background: var(--vp-c-brand);
  color: white;
  border: none;
  padding: 1rem 2rem;
  border-radius: 8px;
  font-weight: 600;
  cursor: pointer;
  font-size: 1rem;
  transition: background 0.2s;
}

.primary-btn:hover {
  background: var(--vp-c-brand-dark);
}

.secondary-btn {
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  border: 1px solid var(--vp-c-border);
  padding: 1rem 2rem;
  border-radius: 8px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.secondary-btn:hover {
  border-color: var(--vp-c-brand);
}

.generator-output {
  background: var(--vp-c-bg-alt);
  border-radius: 8px;
  overflow: hidden;
  margin-top: 2rem;
  border: 1px solid var(--vp-c-border);
}

.output-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 1rem 1.5rem;
  background: var(--vp-c-bg-soft);
  color: var(--vp-c-text-1);
  border-bottom: 1px solid var(--vp-c-border);
}

.output-header h3 {
  margin: 0;
}

.generator-output pre {
  margin: 0;
  padding: 1.5rem;
  overflow-x: auto;
  background: var(--vp-c-bg-alt);
}

.generator-output code {
  font-family: var(--vp-font-family-mono);
  font-size: 0.875rem;
  line-height: 1.5;
  color: var(--vp-c-text-1);
}

.tls-config {
  margin-top: 1rem;
  padding: 1rem;
  background: var(--vp-c-bg);
  border-radius: 6px;
  border: 1px solid var(--vp-c-border);
}

@media (max-width: 768px) {
  .form-row {
    grid-template-columns: 1fr;
  }
  
  .validation-type {
    flex-direction: column;
    gap: 0.5rem;
  }
  
  .generator-actions {
    flex-direction: column;
  }
  
  .examples-grid {
    grid-template-columns: 1fr;
  }
}
</style>

