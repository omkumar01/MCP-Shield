{{/*
  Common labels for all resources
*/}}
{{- define "mcp-shield.labels" -}}
helm.sh/chart: {{ include "mcp-shield.chart" . }}
{{ include "mcp-shield.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- if .Values.global.labels }}
{{- toYaml .Values.global.labels | nindent 4 }}
{{- end }}
{{- end }}

{{/*
  Selector labels for matching pods
*/}}
{{- define "mcp-shield.selectorLabels" -}}
app.kubernetes.io/name: {{ include "mcp-shield.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
  Chart name with version
*/}}
{{- define "mcp-shield.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
  Release name with chart name
*/}}
{{- define "mcp-shield.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
  Service account name
*/}}
{{- define "mcp-shield.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "mcp-shield.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
  Common annotations for resources
*/}}
{{- define "mcp-shield.annotations" -}}
{{- if .Values.global.annotations }}
{{- toYaml .Values.global.annotations | nindent 4 }}
{{- end }}
{{- end }}

{{/*
  Merge dictionaries
*/}}
{{- define "mcp-shield.mergeDicts" -}}
{{- $dict1 := .dict1 -}}
{{- $dict2 := .dict2 -}}
{{- $result := dict -}}
{{- range $k, $v := $dict1 }}
{{- $_ = set $result $k $v }}
{{- end }}
{{- range $k, $v := $dict2 }}
{{- $_ = set $result $k $v }}
{{- end }}
{{- toYaml $result }}
{{- end }}

{{/*
  Render config as TOML
*/}}
{{- define "mcp-shield.configToml" -}}
{{- $config := .Values.config | default dict -}}
{{- $extraConfig := .Values.extraConfig | default dict -}}
{{- $merged := merge $config $extraConfig -}}
{{- toToml $merged }}
{{- end }}

{{/*
  Check if running on OpenShift
*/}}
{{- define "mcp-shield.isOpenShift" -}}
{{- $apiVersions := .Capabilities.APIVersions -}}
{{- $result := false -}}
{{- range $apiVersions }}
{{- if contains "route.openshift.io" . }}
{{- $result = true }}
{{- end }}
{{- end }}
{{- $result }}
{{- end }}

{{/*
  Get image pull secrets
*/}}
{{- define "mcp-shield.imagePullSecrets" -}}
{{- $secrets := list -}}
{{- range .Values.global.imagePullSecrets }}
{{- $secrets = append $secrets (dict "name" .) }}
{{- end }}
{{- range .Values.imagePullSecrets }}
{{- $secrets = append $secrets (dict "name" .) }}
{{- end }}
{{- toYaml $secrets }}
{{- end }}