/* Test-only PAM module: info + error message, a visible PIN prompt and a hidden password prompt
 * in ONE conversation call (multiple messages), then a second call. PIN 4242, password "hunter2". */
#include <security/pam_modules.h>
#include <security/pam_appl.h>
#include <stdlib.h>
#include <string.h>

static int converse(pam_handle_t *h, int n, const struct pam_message **m, struct pam_response **r) {
  const struct pam_conv *c;
  if (pam_get_item(h, PAM_CONV, (const void **)&c) != PAM_SUCCESS || !c) return PAM_CONV_ERR;
  return c->conv(n, m, r, c->appdata_ptr);
}

PAM_EXTERN int pam_sm_authenticate(pam_handle_t *h, int flags, int argc, const char **argv) {
  struct pam_message m1 = {PAM_TEXT_INFO, "Touch the key or enter PIN"};
  struct pam_message m2 = {PAM_ERROR_MSG, "Fingerprint reader unavailable"};
  struct pam_message m3 = {PAM_PROMPT_ECHO_ON, "PIN: "};
  const struct pam_message *batch[] = {&m1, &m2, &m3};
  struct pam_response *r = NULL;
  if (converse(h, 3, batch, &r) != PAM_SUCCESS || !r) return PAM_CONV_ERR;
  int pin_ok = r[2].resp && strcmp(r[2].resp, "4242") == 0;
  for (int i = 0; i < 3; i++) free(r[i].resp);
  free(r);
  struct pam_message m4 = {PAM_PROMPT_ECHO_OFF, "Password: "};
  const struct pam_message *one[] = {&m4};
  r = NULL;
  if (converse(h, 1, one, &r) != PAM_SUCCESS || !r) return PAM_CONV_ERR;
  int pw_ok = r[0].resp && strcmp(r[0].resp, "hunter2") == 0;
  free(r[0].resp);
  free(r);
  return pin_ok && pw_ok ? PAM_SUCCESS : PAM_AUTH_ERR;
}
PAM_EXTERN int pam_sm_setcred(pam_handle_t *h, int f, int c, const char **v) { return PAM_SUCCESS; }
