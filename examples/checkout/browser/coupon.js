// Browser integration seam. The transport is injected by the host application.
export async function submitCoupon(form, transport) {
  const status = form.querySelector('[role="status"]');
  const button = form.querySelector('button[type="submit"]');
  const code = new FormData(form).get("coupon");
  button.disabled = true;
  status.textContent = "Applying coupon…";
  try {
    const result = await transport({ cartId: form.dataset.cartId, code });
    status.textContent = result.accepted
      ? `Discount applied. Total: ${(result.totalCents / 100).toFixed(2)}`
      : result.error;
    return result;
  } catch (error) {
    status.textContent = "Could not reach checkout. Please try again.";
    throw error;
  } finally {
    button.disabled = false;
  }
}
