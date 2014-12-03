package com.github.wartman4404.gldraw.unibrush

import java.io.{File, IOException, InputStream, ByteArrayOutputStream, ByteArrayInputStream, StringReader}
import java.util.zip.{ZipEntry, ZipInputStream}
import android.graphics.Bitmap
import android.util.Log
import android.util.JsonReader

import scala.collection.mutable
import scala.collection.mutable.ArraySeq
import scala.annotation.tailrec

import com.github.wartman4404.gldraw._

import GLResultTypeDef._

case class ShaderSource(
  fragmentshader: Option[String],
  vertexshader: Option[String]
) {
  def compile[T](data: GLInit, compiler: Shader[T], files: Map[String, Array[Byte]]): GLResult[T] = {
    val Seq(frag, vert) = for (path <- Seq(fragmentshader, vertexshader)) yield {
      path.map(x => new String(files.get(x).getOrElse(return UniBrush.logAbort(s"missing shader file ${x}")))).getOrElse(null)
    }
    compiler(data, vert, frag)
  }
}
object ShaderSource {
  def readFromJson(j: JsonReader) = {
    var fragmentshader: Option[String] = None
    var vertexshader: Option[String] = None
    j.beginObject()
      while (j.hasNext()) j.nextName() match {
        case "fragmentshader" => fragmentshader = Some(j.nextString())
        case "vertexshader" => vertexshader = Some(j.nextString())
      }
    j.endObject()
    ShaderSource(fragmentshader, vertexshader)
  }
}

case class LayerSource(
  pointshader: Option[Int],
  copyshader: Option[Int],
  pointsrc: Option[Int]
)

object LayerSource {
  def readFromJson(j: JsonReader) = {
    var pointshader: Option[Int] = None
    var copyshader: Option[Int] = None
    var pointsrc: Option[Int] = None
    j.beginObject()
      while (j.hasNext()) j.nextName() match {
        case "pointshader" => pointshader = Some(j.nextInt())
        case "copyshader" => copyshader = Some(j.nextInt())
        case "pointsrc" => pointsrc = Some(j.nextInt())
      }
    j.endObject()
    LayerSource(pointshader, copyshader, pointsrc)
  }

}

case class Layer(
  pointshader: PointShader,
  copyshader: CopyShader,
  pointsrc: Int
)

case class UniBrushSource (
  brushpath: Option[String],
  pointshaders: Option[Array[ShaderSource]],
  animshaders: Option[Array[ShaderSource]],
  basepointshader: Option[ShaderSource],
  baseanimshader: Option[ShaderSource],
  basecopyshader: Option[ShaderSource],
  interpolator: Option[String],
  layers: Option[Array[LayerSource]]
)
object UniBrushSource extends AndroidImplicits {
  def readFromJson(j: JsonReader) = {
    var brushpath: Option[String] = None
    var pointshaders: Option[Array[ShaderSource]] = None
    var animshaders: Option[Array[ShaderSource]] = None
    var basepointshader: Option[ShaderSource] = None
    var baseanimshader: Option[ShaderSource] = None
    var basecopyshader: Option[ShaderSource] = None
    var interpolator: Option[String] = None
    var layers: Option[Array[LayerSource]] = None
    j.beginObject()
      while (j.hasNext()) j.nextName() match {
        case "brushpath" => brushpath = Some(j.nextString())
        case "pointshaders" => pointshaders = Some(j.readArray(ShaderSource.readFromJson).toArray)
        case "animshaders" => animshaders = Some(j.readArray(ShaderSource.readFromJson).toArray)
        case "basepointshader" => basepointshader = Some(ShaderSource.readFromJson(j))
        case "baseanimshader" => baseanimshader = Some(ShaderSource.readFromJson(j))
        case "basecopyshader" => basecopyshader = Some(ShaderSource.readFromJson(j))
        case "interpolator" => interpolator = Some(j.nextString())
        case "layers" => layers = Some(j.readArray(LayerSource.readFromJson).toArray)
      }
    j.endObject()
    UniBrushSource(brushpath, pointshaders, animshaders, basepointshader,
      baseanimshader, basecopyshader, interpolator, layers)
  }
}

case class UniBrush(
  brush: Option[Texture],
  basepointshader: Option[PointShader],
  baseanimshader: Option[CopyShader],
  basecopyshader: Option[CopyShader],
  interpolator: Option[LuaScript],
  layers: Array[Layer])

object UniBrush {
  def logAbort[T](s: String): GLResult[T] = {
    Log.e("unibrush", s"failed to load: ${s}")
    throw new GLException(s)
  }

  // iterator to unzip everything into memory
  // this is incredibly wasteful, even more so because the files still have to
  // be converted to strings/bitmaps
  // it would be way better to read the compressed zipfile into memory instead
  // but that involves third-party libraries and looks fussy
  private implicit class ZipInputStream2Iterator(zis: ZipInputStream) extends Iterable[(ZipEntry, Array[Byte])] {
    def iterator = new ZipInputStreamIterator(zis)
  }
  class ZipInputStreamIterator(zis: ZipInputStream) extends Iterator[(ZipEntry, Array[Byte])] {
    private var nextEntry = zis.getNextEntry()
    private var baos = new ByteArrayOutputStream()
    private val ba = new Array[Byte](8192)
    def hasNext =
      if (nextEntry == null) { zis.close(); false }
      else true

    @tailrec final def next(): (ZipEntry, Array[Byte]) = {
      val readBytes = zis.read(ba, 0, ba.length)
      if (readBytes == -1) {
        baos.flush()
        val oldBytes = baos.toByteArray()
        val oldEntry = nextEntry
        zis.closeEntry()
        nextEntry = zis.getNextEntry()
        baos = new ByteArrayOutputStream()
        Log.i("unibrush", s"read ${oldEntry.getName()}: ${oldBytes.length} bytes")
        (oldEntry, oldBytes)
      } else {
        baos.write(ba, 0, readBytes)
        this.next()
      }
    }
  }

  def compileFromStream(data: GLInit, sourceZip: InputStream): GLResult[UniBrush] = {
    Log.i("unibrush", "loading unibrush")
    try {
      val files = new ZipInputStream(sourceZip)
        .map { case (entry, bytes) => (entry.getName(), bytes) }
        .toMap
      val brushjson = files.get("brush.json").getOrElse(return logAbort("unable to find brush.json"))
      Log.i("unibrush", "got brush.json")
      val brushjsonreader = new JsonReader(new StringReader(new String(brushjson)))
      compile(data, UniBrushSource.readFromJson(brushjsonreader), files)
    } catch {
      case e: IOException => logAbort(s"Error reading unibrush ${e}")
      case e: GLException => logAbort(s"Error in unibrush files ${e}")
      case e: Exception => logAbort(s"Other exception ${e}")
    }
  }

  def compileShaders[T](data: GLInit, shaders: Option[Array[ShaderSource]], compiler: Shader[T], files: Map[String, Array[Byte]]): GLResult[ArraySeq[T]] = {
    shaders.getOrElse(Array.empty).map(x => x.compile(data, compiler, files))
  }

  def getLayers(data: GLInit, pointshaders: Array[PointShader], copyshaders: Array[CopyShader], layers: Option[Array[LayerSource]]): GLResult[Array[Layer]] = {
    layers.getOrElse(Array.empty).map(l => {
      val point = l.pointshader.map(x => pointshaders.lift(x).getOrElse(logAbort(s"no point shader numbered ${x}"))).getOrElse(PointShader(data, null, null))
      val copy = l.copyshader.map(x => copyshaders.lift(x).getOrElse(logAbort(s"no copy shader numbered ${x}"))).getOrElse(CopyShader(data, null, null))
      val idx = l.pointsrc.getOrElse(0)
      Layer(point, copy, idx)
    })
  }

  def flipoption[T,U,V](opt: Option[T], cb: (T)=>Either[U,V]): Either[U,Option[V]] = {
    opt match {
      case None => Right(None)
      case Some(x) => cb(x) match {
        case Left(y) => Left(y)
        case Right(z) => Right(Some(z))
      }
    }
  }


  def compile(data: GLInit, s: UniBrushSource, files: Map[String, Array[Byte]]): GLResult[UniBrush] = {
    Log.i("unibrush", "compiling unibrush");
    val brush = s.brushpath.map(bp => {
        val stream = (files.get(bp)
        .map(new ByteArrayInputStream(_))
        .getOrElse(logAbort(s"unable to load bitmap in unibrush: ${bp}")))
        Texture(data, DrawFiles.decodeBitmap(Bitmap.Config.ALPHA_8)(stream))
    })
    val pointshaders: GLResult[ArraySeq[PointShader]] = compileShaders(data, s.pointshaders, PointShader, files)
    val copyshaders = compileShaders(data, s.animshaders, CopyShader, files)
    val baseanimshader = s.baseanimshader.map(_.compile(data, CopyShader, files))
    val basecopyshader = s.basecopyshader.map(_.compile(data, CopyShader, files))
    val basepointshader = s.basepointshader.map(_.compile(data, PointShader, files))
    val interpolator = s.interpolator.map(path => {
      val luastring = files.get(path).getOrElse(logAbort(s"missing interpolator file ${path}"))
      LuaScript(data, new String(luastring))
    })
    val layers = getLayers(data, pointshaders.toArray, copyshaders.toArray, s.layers)
    Log.i("unibrush", s"have interpolator: ${interpolator.nonEmpty}");
    Log.i("unibrush", s"have pointshader: ${basepointshader.nonEmpty}");
    Log.i("unibrush", s"have animshader: ${baseanimshader.nonEmpty}");
    Log.i("unibrush", s"have layers: ${layers.length}");
    Log.i("unibrush", s"have brush: ${brush.nonEmpty}");
    UniBrush(brush, basepointshader, baseanimshader, basecopyshader, interpolator, layers)
  }
}
