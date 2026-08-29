import { useEffect, useRef } from "react";
import { useReducedMotion } from "../hooks/useReducedMotion";

type TimeTheme = "light" | "dark";

export function SyncField({ theme }: { theme: TimeTheme }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const reducedMotion = useReducedMotion();

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const context = canvas.getContext("2d");
    if (!context) return;

    const pointer = { x: -1000, y: -1000, active: false };
    const lightTheme = theme === "light";
    // Warm atelier palette: coral / warm-orange / amber / warm ink.
    const colors = lightTheme
      ? ["#bd4f2c", "#c8703c", "#c88a3c", "#17160f"]
      : ["#f0946a", "#f0a884", "#c88a3c", "#f2ebdb"];
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
    let elapsedWhenPaused = 0;

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
        ? lightTheme ? "rgba(200,112,60,.7)" : "rgba(200,138,60,.75)"
        : lightTheme ? "rgba(189,79,44,.72)" : "rgba(240,148,106,.78)";
      context.lineWidth = 1.2;
      context.beginPath();
      context.arc(0, 0, 30 + pulse, 0, Math.PI * 2);
      context.stroke();
      context.strokeStyle = lightTheme
        ? "rgba(23,22,15,.12)"
        : "rgba(242,235,219,.12)";
      context.lineWidth = 1;
      context.beginPath();
      context.arc(0, 0, 46 - pulse, 0, Math.PI * 2);
      context.stroke();
      context.fillStyle = reverse
        ? lightTheme ? "#c8703c" : "#c88a3c"
        : lightTheme ? "#bd4f2c" : "#f0946a";
      context.beginPath();
      context.arc(0, 0, 3.5, 0, Math.PI * 2);
      context.fill();
      context.restore();
    };

    const draw = (time: number) => {
      context.clearRect(0, 0, width, height);
      const elapsed = reducedMotion ? 0 : time - start;
      elapsedWhenPaused = elapsed;
      const leftX = Math.max(88, width * 0.16);
      const rightX = Math.min(width - 88, width * 0.84);
      const centerY = height * 0.54;

      context.save();
      context.strokeStyle = lightTheme
        ? "rgba(23,22,15,.045)"
        : "rgba(242,235,219,.04)";
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
          ? lightTheme ? "rgba(189,79,44,.26)" : "rgba(240,148,106,.24)"
          : lightTheme ? "rgba(23,22,15,.08)" : "rgba(242,235,219,.08)";
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
          ? "rgba(23,22,15,.2)"
          : "rgba(242,235,219,.22)";
        context.stroke();
      }

      if (!reducedMotion) frame = window.requestAnimationFrame(draw);
    };

    resize();
    window.addEventListener("resize", resize);
    canvas.addEventListener("pointermove", onPointerMove);
    canvas.addEventListener("pointerleave", onPointerLeave);

    // Under reduced motion every frame is identical, so draw once and stop
    // rather than re-rasterising a static image forever.
    if (reducedMotion) {
      frame = window.requestAnimationFrame(draw);
      return () => {
        window.removeEventListener("resize", resize);
        canvas.removeEventListener("pointermove", onPointerMove);
        canvas.removeEventListener("pointerleave", onPointerLeave);
        window.cancelAnimationFrame(frame);
      };
    }

    // The hero fills the first viewport, so once the reader is past it the
    // particle field is painting 52 arcs a frame that nobody can see.
    let onStage = true;
    let hidden = document.visibilityState !== "visible";

    const stop = () => {
      window.cancelAnimationFrame(frame);
      frame = 0;
    };
    const play = () => {
      if (frame || !onStage || hidden) return;
      // Rebase the clock so the field resumes where it left off instead of
      // jumping forward by however long it was parked.
      start = performance.now() - elapsedWhenPaused;
      frame = window.requestAnimationFrame(draw);
    };

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) onStage = entry.isIntersecting;
        if (onStage) play();
        else stop();
      },
      { threshold: 0 },
    );
    observer.observe(canvas);

    const handleVisibilityChange = () => {
      hidden = document.visibilityState !== "visible";
      if (hidden) stop();
      else play();
    };
    document.addEventListener("visibilitychange", handleVisibilityChange);

    play();

    return () => {
      window.removeEventListener("resize", resize);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      observer.disconnect();
      stop();
    };
  }, [reducedMotion, theme]);

  return <canvas ref={canvasRef} className="sync-field" aria-hidden="true" />;
}
