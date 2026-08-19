import { useEffect, useRef } from "react";

type TimeTheme = "light" | "dark";

export function SyncField({ theme }: { theme: TimeTheme }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const context = canvas.getContext("2d");
    if (!context) return;

    const reducedMotion = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    const pointer = { x: -1000, y: -1000, active: false };
    const lightTheme = theme === "light";
    const colors = lightTheme
      ? ["#0071e3", "#7d5aff", "#ff5e8a", "#1d1d1f"]
      : ["#2997ff", "#bf5af2", "#ff6482", "#f5f5f7"];
    const particles = Array.from({ length: 52 }, (_, index) => ({
      offset: index / 52,
      lane: (index % 7) - 3,
      speed: 0.000035 + (index % 5) * 0.000004,
      size: 1.4 + (index % 4) * 0.7,
      color: colors[index % colors.length],
    }));

    let width = 0;
    let height = 0;
    let frame = 0;
    let start = performance.now();

    const resize = () => {
      const rect = canvas.getBoundingClientRect();
      const ratio = Math.min(window.devicePixelRatio || 1, 2);
      width = rect.width;
      height = rect.height;
      canvas.width = Math.max(1, Math.round(width * ratio));
      canvas.height = Math.max(1, Math.round(height * ratio));
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
    };

    const onPointerMove = (event: PointerEvent) => {
      const rect = canvas.getBoundingClientRect();
      pointer.x = event.clientX - rect.left;
      pointer.y = event.clientY - rect.top;
      pointer.active = true;
    };

    const onPointerLeave = () => {
      pointer.active = false;
    };

    const cubicPoint = (
      t: number,
      startX: number,
      startY: number,
      controlX1: number,
      controlY1: number,
      controlX2: number,
      controlY2: number,
      endX: number,
      endY: number,
    ) => {
      const inverse = 1 - t;
      return {
        x:
          inverse ** 3 * startX +
          3 * inverse ** 2 * t * controlX1 +
          3 * inverse * t ** 2 * controlX2 +
          t ** 3 * endX,
        y:
          inverse ** 3 * startY +
          3 * inverse ** 2 * t * controlY1 +
          3 * inverse * t ** 2 * controlY2 +
          t ** 3 * endY,
      };
    };

    const drawEndpoint = (x: number, y: number, time: number, reverse = false) => {
      const pulse = reducedMotion ? 0 : Math.sin(time * 0.002 + (reverse ? 2 : 0)) * 4;
      context.save();
      context.translate(x, y);
      context.strokeStyle = reverse
        ? lightTheme ? "rgba(125,90,255,.7)" : "rgba(191,90,242,.75)"
        : lightTheme ? "rgba(0,113,227,.72)" : "rgba(41,151,255,.78)";
      context.lineWidth = 1.2;
      context.beginPath();
      context.arc(0, 0, 30 + pulse, 0, Math.PI * 2);
      context.stroke();
      context.strokeStyle = lightTheme
        ? "rgba(29,29,31,.12)"
        : "rgba(245,245,247,.12)";
      context.lineWidth = 1;
      context.beginPath();
      context.arc(0, 0, 46 - pulse, 0, Math.PI * 2);
      context.stroke();
      context.fillStyle = reverse
        ? lightTheme ? "#7d5aff" : "#bf5af2"
        : lightTheme ? "#0071e3" : "#2997ff";
      context.beginPath();
      context.arc(0, 0, 3.5, 0, Math.PI * 2);
      context.fill();
      context.restore();
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, width, height);
      const elapsed = reducedMotion ? 0 : time - start;
      const leftX = Math.max(88, width * 0.16);
      const rightX = Math.min(width - 88, width * 0.84);
      const centerY = height * 0.54;

      context.save();
      context.strokeStyle = lightTheme
        ? "rgba(29,29,31,.045)"
        : "rgba(245,245,247,.04)";
      context.lineWidth = 1;
      for (let x = 0; x < width; x += 72) {
        context.beginPath();
        context.moveTo(x, 0);
        context.lineTo(x, height);
        context.stroke();
      }
      for (let y = 0; y < height; y += 72) {
        context.beginPath();
        context.moveTo(0, y);
        context.lineTo(width, y);
        context.stroke();
      }
      context.restore();

      for (let lane = -3; lane <= 3; lane += 1) {
        const bend = lane * 28;
        context.beginPath();
        context.moveTo(leftX, centerY + lane * 8);
        context.bezierCurveTo(
          width * 0.36,
          centerY - 150 + bend,
          width * 0.64,
          centerY + 150 + bend,
          rightX,
          centerY - lane * 8,
        );
        context.strokeStyle = lane === 0
          ? lightTheme ? "rgba(0,113,227,.26)" : "rgba(41,151,255,.24)"
          : lightTheme ? "rgba(29,29,31,.08)" : "rgba(245,245,247,.08)";
        context.lineWidth = lane === 0 ? 1.4 : 0.8;
        context.stroke();
      }

      context.globalCompositeOperation = "lighter";
      particles.forEach((particle) => {
        const progress = reducedMotion
          ? particle.offset
          : (particle.offset + elapsed * particle.speed) % 1;
        const laneOffset = particle.lane * 8;
        const point = cubicPoint(
          progress,
          leftX,
          centerY + laneOffset,
          width * 0.36,
          centerY - 150 + particle.lane * 28,
          width * 0.64,
          centerY + 150 + particle.lane * 28,
          rightX,
          centerY - laneOffset,
        );
        let drawX = point.x;
        let drawY = point.y;

        if (pointer.active) {
          const deltaX = drawX - pointer.x;
          const deltaY = drawY - pointer.y;
          const distance = Math.hypot(deltaX, deltaY);
          if (distance < 110 && distance > 0) {
            const force = (110 - distance) / 110;
            drawX += (deltaX / distance) * force * 24;
            drawY += (deltaY / distance) * force * 24;
          }
        }

        const alpha = Math.sin(progress * Math.PI) * 0.92;
        context.globalAlpha = Math.max(0.12, alpha);
        context.fillStyle = particle.color;
        context.beginPath();
        context.arc(drawX, drawY, particle.size, 0, Math.PI * 2);
        context.fill();
      });
      context.globalAlpha = 1;
      context.globalCompositeOperation = "source-over";

      drawEndpoint(leftX, centerY, time);
      drawEndpoint(rightX, centerY, time, true);

      if (pointer.active) {
        context.beginPath();
        context.arc(pointer.x, pointer.y, 18, 0, Math.PI * 2);
        context.strokeStyle = lightTheme
          ? "rgba(29,29,31,.2)"
          : "rgba(245,245,247,.22)";
        context.stroke();
      }

      frame = window.requestAnimationFrame(draw);
    };

    resize();
    window.addEventListener("resize", resize);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerleave", onPointerLeave);
    frame = window.requestAnimationFrame(draw);

    return () => {
      window.removeEventListener("resize", resize);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      window.cancelAnimationFrame(frame);
      start = 0;
    };
  }, [theme]);

  return <canvas ref={canvasRef} className="sync-field" aria-hidden="true" />;
}
